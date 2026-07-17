//! Substitution environment: the variable bindings built during unification.
//!
//! A [`LogicEnv`] maps logic variables to their bound terms and drives the
//! unifier. It bridges to the kernel `Shape` contracts by projecting its
//! bindings into a `ShapeMatch`; see the [`README`](https://docs.rs/sim-runtime).
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sim_kernel::{Cx, Expr, MatchScore, Result, ShapeBindings, ShapeMatch, Symbol};
use sim_shape::{AnyShape, CaptureShape, ExactExprShape, ListShape, Shape, ShapeObject};

use crate::model::OccursCheck;

/// A unification substitution: bindings from logic variables to terms.
///
/// Carries a resolution `depth` so renamed clause variables stay distinct
/// across recursive calls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogicEnv {
    captures: BTreeMap<Symbol, Expr>,
    depth: usize,
}

impl LogicEnv {
    /// Creates an empty environment at depth zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty environment recorded at the given resolution depth.
    pub fn with_depth(depth: usize) -> Self {
        Self {
            captures: BTreeMap::new(),
            depth,
        }
    }

    /// Returns the resolution depth recorded for this environment.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Sets the recorded resolution depth.
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Applies the substitution to `expr`, recursively replacing bound
    /// variables with their values.
    pub fn apply(&self, expr: &Expr) -> Expr {
        match expr {
            Expr::Local(var) => match self.captures.get(var) {
                Some(bound) => self.apply(bound),
                None => Expr::Local(var.clone()),
            },
            Expr::List(items) => Expr::List(items.iter().map(|item| self.apply(item)).collect()),
            Expr::Vector(items) => {
                Expr::Vector(items.iter().map(|item| self.apply(item)).collect())
            }
            Expr::Map(entries) => Expr::Map(
                entries
                    .iter()
                    .map(|(key, value)| (self.apply(key), self.apply(value)))
                    .collect(),
            ),
            Expr::Set(items) => Expr::Set(items.iter().map(|item| self.apply(item)).collect()),
            Expr::Call { operator, args } => Expr::Call {
                operator: Box::new(self.apply(operator)),
                args: args.iter().map(|arg| self.apply(arg)).collect(),
            },
            Expr::Infix {
                operator,
                left,
                right,
            } => Expr::Infix {
                operator: operator.clone(),
                left: Box::new(self.apply(left)),
                right: Box::new(self.apply(right)),
            },
            Expr::Prefix { operator, arg } => Expr::Prefix {
                operator: operator.clone(),
                arg: Box::new(self.apply(arg)),
            },
            Expr::Postfix { operator, arg } => Expr::Postfix {
                operator: operator.clone(),
                arg: Box::new(self.apply(arg)),
            },
            Expr::Block(items) => Expr::Block(items.iter().map(|item| self.apply(item)).collect()),
            Expr::Quote { mode, expr } => Expr::Quote {
                mode: *mode,
                expr: Box::new(self.apply(expr)),
            },
            Expr::Annotated { expr, annotations } => Expr::Annotated {
                expr: Box::new(self.apply(expr)),
                annotations: annotations
                    .iter()
                    .map(|(name, value)| (name.clone(), self.apply(value)))
                    .collect(),
            },
            Expr::Extension { tag, payload } => Expr::Extension {
                tag: tag.clone(),
                payload: Box::new(self.apply(payload)),
            },
            other => other.clone(),
        }
    }

    /// Returns the term directly bound to `var`, if any.
    pub fn get(&self, var: &Symbol) -> Option<&Expr> {
        self.captures.get(var)
    }

    /// Binds `var` to `value`, honoring the [`OccursCheck`] policy.
    ///
    /// Returns an error when an enabled occurs check detects that `var` occurs
    /// in `value` (which would build a cyclic term).
    pub fn bind(&mut self, var: Symbol, value: Expr, occurs_check: OccursCheck) -> Result<()> {
        if matches!(occurs_check, OccursCheck::Always) && occurs(var.clone(), &value, self) {
            return Err(sim_kernel::Error::Eval(format!(
                "occurs check failed for ?{}",
                var.name
            )));
        }
        self.captures.insert(var, value);
        Ok(())
    }

    /// Unifies two terms, extending the substitution in place.
    ///
    /// Returns `true` when the terms unify and `false` on a structural
    /// mismatch; errors propagate only from a failed occurs check.
    pub fn unify(
        &mut self,
        cx: &mut Cx,
        left: &Expr,
        right: &Expr,
        occurs_check: OccursCheck,
    ) -> Result<bool> {
        let left = self.apply(left);
        let right = self.apply(right);
        if left.canonical_eq(&right) {
            return Ok(true);
        }

        let left_match = self.shape_unify(cx, &left, &right, occurs_check)?;
        let right_match = self.shape_unify(cx, &right, &left, occurs_check)?;
        match (left_match, right_match) {
            (ShapeUnify::Accepted(next), _) | (_, ShapeUnify::Accepted(next)) => {
                *self = next;
                Ok(true)
            }
            (ShapeUnify::Unsupported, _) | (_, ShapeUnify::Unsupported) => {
                unify_ground(cx, self, &left, &right, occurs_check)
            }
            (ShapeUnify::Rejected, ShapeUnify::Rejected) => Ok(false),
        }
    }

    fn shape_unify(
        &self,
        cx: &mut Cx,
        pattern: &Expr,
        subject: &Expr,
        occurs_check: OccursCheck,
    ) -> Result<ShapeUnify> {
        let Some(shape) = shape_from_pattern(cx, pattern) else {
            return Ok(ShapeUnify::Unsupported);
        };
        let matched = shape.check_expr(cx, subject)?;
        if !matched.accepted {
            return Ok(ShapeUnify::Rejected);
        }
        let mut next = self.clone();
        if next.merge_shape_captures(cx, &matched.captures, occurs_check)? {
            Ok(ShapeUnify::Accepted(next))
        } else {
            Ok(ShapeUnify::Rejected)
        }
    }

    fn merge_shape_captures(
        &mut self,
        cx: &mut Cx,
        captures: &ShapeBindings,
        occurs_check: OccursCheck,
    ) -> Result<bool> {
        for (var, value) in captures.exprs() {
            if !self.merge_shape_capture(cx, var.clone(), value.clone(), occurs_check)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn merge_shape_capture(
        &mut self,
        cx: &mut Cx,
        var: Symbol,
        value: Expr,
        occurs_check: OccursCheck,
    ) -> Result<bool> {
        let value = self.apply(&value);
        if let Some(bound) = self.captures.get(&var).cloned() {
            let bound = self.apply(&bound);
            return self.unify(cx, &bound, &value, occurs_check);
        }
        self.bind(var, value, occurs_check)?;
        Ok(true)
    }

    /// Collects the distinct logic variables appearing in `expr`.
    pub fn free_vars(&self, expr: &Expr) -> Vec<Symbol> {
        let mut vars = BTreeSet::new();
        collect_vars(expr, &mut vars);
        vars.into_iter().collect()
    }

    /// Projects the current bindings into kernel `ShapeBindings`.
    pub fn to_shape_bindings(&self, _cx: &mut Cx) -> Result<ShapeBindings> {
        let mut bindings = ShapeBindings::new();
        for (name, expr) in &self.captures {
            bindings.bind_expr(name.clone(), self.apply(expr));
        }
        Ok(bindings)
    }

    /// Builds an accepting kernel `ShapeMatch` whose captures are this
    /// environment's bindings.
    pub fn as_shape_match(&self, cx: &mut Cx) -> Result<ShapeMatch> {
        Ok(ShapeMatch {
            accepted: true,
            captures: self.to_shape_bindings(cx)?,
            score: MatchScore::exact(100),
            diagnostics: Vec::new(),
        })
    }
}

enum ShapeUnify {
    Accepted(LogicEnv),
    Rejected,
    Unsupported,
}

fn shape_from_pattern(cx: &mut Cx, pattern: &Expr) -> Option<Arc<dyn Shape>> {
    match pattern {
        Expr::Local(var) => Some(Arc::new(CaptureShape::new(var.clone(), Arc::new(AnyShape)))),
        Expr::List(items) => {
            let item_shapes = items
                .iter()
                .map(|item| shape_from_pattern(cx, item))
                .collect::<Option<Vec<_>>>()?;
            Some(Arc::new(ListShape::new(item_shapes)))
        }
        Expr::Symbol(symbol) => resolve_shape_symbol(cx, symbol)
            .or_else(|| Some(Arc::new(ExactExprShape::new(pattern.clone())))),
        other if !contains_local(other) => Some(Arc::new(ExactExprShape::new(other.clone()))),
        _ => None,
    }
}

fn resolve_shape_symbol(cx: &mut Cx, symbol: &Symbol) -> Option<Arc<dyn Shape>> {
    let value = cx.resolve_shape(symbol).ok()?;
    let shape = value.object().downcast_ref::<ShapeObject>()?;
    Some(Arc::clone(&shape.shape))
}

fn unify_ground(
    cx: &mut Cx,
    env: &mut LogicEnv,
    left: &Expr,
    right: &Expr,
    occurs_check: OccursCheck,
) -> Result<bool> {
    match (left, right) {
        (Expr::Nil, Expr::Nil)
        | (Expr::Bool(_), Expr::Bool(_))
        | (Expr::Number(_), Expr::Number(_))
        | (Expr::Symbol(_), Expr::Symbol(_))
        | (Expr::Local(_), Expr::Local(_))
        | (Expr::String(_), Expr::String(_))
        | (Expr::Bytes(_), Expr::Bytes(_)) => Ok(left.canonical_eq(right)),
        (Expr::List(left_items), Expr::List(right_items))
        | (Expr::Vector(left_items), Expr::Vector(right_items))
        | (Expr::Set(left_items), Expr::Set(right_items))
        | (Expr::Block(left_items), Expr::Block(right_items)) => {
            unify_slices(cx, env, left_items, right_items, occurs_check)
        }
        (Expr::Map(left_entries), Expr::Map(right_entries)) => {
            if left_entries.len() != right_entries.len() {
                return Ok(false);
            }
            for ((left_key, left_value), (right_key, right_value)) in
                left_entries.iter().zip(right_entries.iter())
            {
                if !env.unify(cx, left_key, right_key, occurs_check)? {
                    return Ok(false);
                }
                if !env.unify(cx, left_value, right_value, occurs_check)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (
            Expr::Call {
                operator: left_op,
                args: left_args,
            },
            Expr::Call {
                operator: right_op,
                args: right_args,
            },
        ) => {
            if left_args.len() != right_args.len()
                || !env.unify(cx, left_op, right_op, occurs_check)?
            {
                return Ok(false);
            }
            unify_slices(cx, env, left_args, right_args, occurs_check)
        }
        (
            Expr::Quote {
                mode: left_mode,
                expr: left_expr,
            },
            Expr::Quote {
                mode: right_mode,
                expr: right_expr,
            },
        ) => {
            if left_mode != right_mode {
                return Ok(false);
            }
            env.unify(cx, left_expr, right_expr, occurs_check)
        }
        (
            Expr::Annotated {
                expr: left_expr,
                annotations: left_annotations,
            },
            Expr::Annotated {
                expr: right_expr,
                annotations: right_annotations,
            },
        ) => {
            if left_annotations.len() != right_annotations.len()
                || !env.unify(cx, left_expr, right_expr, occurs_check)?
            {
                return Ok(false);
            }
            for ((left_name, left_value), (right_name, right_value)) in
                left_annotations.iter().zip(right_annotations.iter())
            {
                if left_name != right_name
                    || !env.unify(cx, left_value, right_value, occurs_check)?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (
            Expr::Extension {
                tag: left_tag,
                payload: left_payload,
            },
            Expr::Extension {
                tag: right_tag,
                payload: right_payload,
            },
        ) => Ok(left_tag == right_tag && env.unify(cx, left_payload, right_payload, occurs_check)?),
        (
            Expr::Infix {
                operator: left_op,
                left: left_a,
                right: left_b,
            },
            Expr::Infix {
                operator: right_op,
                left: right_a,
                right: right_b,
            },
        ) => Ok(left_op == right_op
            && env.unify(cx, left_a, right_a, occurs_check)?
            && env.unify(cx, left_b, right_b, occurs_check)?),
        (
            Expr::Prefix {
                operator: left_op,
                arg: left_arg,
            },
            Expr::Prefix {
                operator: right_op,
                arg: right_arg,
            },
        )
        | (
            Expr::Postfix {
                operator: left_op,
                arg: left_arg,
            },
            Expr::Postfix {
                operator: right_op,
                arg: right_arg,
            },
        ) => Ok(left_op == right_op && env.unify(cx, left_arg, right_arg, occurs_check)?),
        _ => Ok(false),
    }
}

fn unify_slices(
    cx: &mut Cx,
    env: &mut LogicEnv,
    left: &[Expr],
    right: &[Expr],
    occurs_check: OccursCheck,
) -> Result<bool> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left_item, right_item) in left.iter().zip(right.iter()) {
        if !env.unify(cx, left_item, right_item, occurs_check)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn occurs(var: Symbol, expr: &Expr, env: &LogicEnv) -> bool {
    match env.apply(expr) {
        Expr::Local(candidate) => candidate == var,
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().any(|item| occurs(var.clone(), item, env))
        }
        Expr::Map(entries) => entries
            .iter()
            .any(|(key, value)| occurs(var.clone(), key, env) || occurs(var.clone(), value, env)),
        Expr::Call { operator, args } => {
            occurs(var.clone(), &operator, env)
                || args.iter().any(|arg| occurs(var.clone(), arg, env))
        }
        Expr::Infix { left, right, .. } => {
            occurs(var.clone(), &left, env) || occurs(var, &right, env)
        }
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => occurs(var, &arg, env),
        Expr::Quote { expr, .. } => occurs(var, &expr, env),
        Expr::Annotated { expr, annotations } => {
            occurs(var.clone(), &expr, env)
                || annotations
                    .iter()
                    .any(|(_, value)| occurs(var.clone(), value, env))
        }
        Expr::Extension { payload, .. } => occurs(var, &payload, env),
        _ => false,
    }
}

fn contains_local(expr: &Expr) -> bool {
    match expr {
        Expr::Local(_) => true,
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().any(contains_local)
        }
        Expr::Map(entries) => entries
            .iter()
            .any(|(key, value)| contains_local(key) || contains_local(value)),
        Expr::Call { operator, args } => {
            contains_local(operator) || args.iter().any(contains_local)
        }
        Expr::Infix { left, right, .. } => contains_local(left) || contains_local(right),
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => contains_local(arg),
        Expr::Quote { expr, .. } => contains_local(expr),
        Expr::Annotated { expr, annotations } => {
            contains_local(expr) || annotations.iter().any(|(_, value)| contains_local(value))
        }
        Expr::Extension { payload, .. } => contains_local(payload),
        _ => false,
    }
}

fn collect_vars(expr: &Expr, vars: &mut BTreeSet<Symbol>) {
    match expr {
        Expr::Local(var) => {
            vars.insert(var.clone());
        }
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            for item in items {
                collect_vars(item, vars);
            }
        }
        Expr::Map(entries) => {
            for (key, value) in entries {
                collect_vars(key, vars);
                collect_vars(value, vars);
            }
        }
        Expr::Call { operator, args } => {
            collect_vars(operator, vars);
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Infix { left, right, .. } => {
            collect_vars(left, vars);
            collect_vars(right, vars);
        }
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => collect_vars(arg, vars),
        Expr::Quote { expr, .. } => collect_vars(expr, vars),
        Expr::Annotated { expr, annotations } => {
            collect_vars(expr, vars);
            for (_, value) in annotations {
                collect_vars(value, vars);
            }
        }
        Expr::Extension { payload, .. } => collect_vars(payload, vars),
        _ => {}
    }
}
