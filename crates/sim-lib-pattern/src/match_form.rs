//! The `match` pattern special form.
//!
//! `match` is an eval-policy organ. Like the other
//! control/binding organs it is a special form -- a [`Callable`] overriding
//! [`Callable::call_exprs`] so it receives its arguments UNEVALUATED. It
//! evaluates the scrutinee once, then tries each clause's pattern in order via
//! the kernel [`Shape`] match/binding protocol ([`match_value`]); the first arm
//! whose pattern accepts the value has its captures installed into a fresh child
//! [`Env`](sim_kernel::Env) and its body evaluated there.
//!
//! Supported patterns (compiled to kernel shapes):
//! - `_`            -> [`AnyShape`] (wildcard, binds nothing),
//! - a symbol `x`   -> [`CaptureShape`] over [`AnyShape`] (binds the value to `x`),
//! - a literal      -> [`ExactExprShape`] (matches an equal number/string/bool/nil),
//! - `[p ...]`      -> [`ListShape`] of the element patterns (list/vector destructure,
//!   composing the element captures).
//!
//! Constructor and ADT data are handled by the crate's
//! `AlgebraicDataType`/`VariantConstructor` machinery; this form accepts the
//! shape patterns listed above.

use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, RawArgs, Result, Shape,
    Symbol, Value,
};
use sim_shape::{AnyShape, CaptureShape, ExactExprShape, ListShape};

use crate::matching::{MatchArm, match_value};

/// The `match` special form: `(match scrutinee (pattern body...) ...)`.
#[derive(Clone, Copy)]
pub struct MatchForm;

impl MatchForm {
    /// The bare `match` symbol this form registers under.
    pub fn symbol() -> Symbol {
        Symbol::new("match")
    }
}

/// Compile a pattern expression into a checking/binding kernel [`Shape`].
fn compile_pattern(pattern: Expr) -> Result<Arc<dyn Shape>> {
    match pattern {
        Expr::Symbol(sym) if sym.namespace.is_none() && sym.name.as_ref() == "_" => {
            Ok(Arc::new(AnyShape))
        }
        Expr::Symbol(sym) => Ok(Arc::new(CaptureShape::new(sym, Arc::new(AnyShape)))),
        literal @ (Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Nil) => {
            Ok(Arc::new(ExactExprShape::new(literal)))
        }
        Expr::List(items) | Expr::Vector(items) => {
            let shapes = items
                .into_iter()
                .map(compile_pattern)
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(ListShape::new(shapes)))
        }
        other => Err(Error::Eval(format!(
            "unsupported match pattern: {other:?} (patterns are `_`, a symbol, a literal, or a `[..]` list)"
        ))),
    }
}

/// Split a clause into its pattern and body expressions.
fn clause_parts(clause: Expr) -> Result<(Expr, Vec<Expr>)> {
    let items = match clause {
        Expr::List(items) | Expr::Vector(items) => items,
        Expr::Call { operator, args } => {
            let mut items = vec![*operator];
            items.extend(args);
            items
        }
        other => {
            return Err(Error::Eval(format!(
                "match clause must be (pattern body...), got {other:?}"
            )));
        }
    };
    let mut items = items.into_iter();
    let pattern = items
        .next()
        .ok_or_else(|| Error::Eval("match clause needs a pattern".to_owned()))?;
    Ok((pattern, items.collect()))
}

impl Object for MatchForm {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<special-form match>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MatchForm {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for MatchForm {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(Error::Eval(
            "match is a special form and cannot be applied to evaluated arguments".to_owned(),
        ))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let mut exprs = args.into_exprs().into_iter();
        let Some(scrutinee) = exprs.next() else {
            return Err(Error::Eval(
                "match expects (match scrutinee (pattern body...) ...)".to_owned(),
            ));
        };

        // Compile every clause's pattern into an arm, keeping its body alongside.
        let mut arms = Vec::new();
        let mut bodies = Vec::new();
        for (index, clause) in exprs.enumerate() {
            let (pattern, body) = clause_parts(clause)?;
            arms.push(MatchArm::new(
                Symbol::new(format!("arm-{index}")),
                compile_pattern(pattern)?,
            ));
            bodies.push(body);
        }

        let value = cx.eval_expr(scrutinee)?;
        let matched = match_value(cx, value, &arms)?;
        let body = bodies
            .into_iter()
            .nth(matched.arm_index())
            .expect("arm index within bodies");

        let child = matched.captures().clone().into_child_env(cx)?;
        cx.with_env(child, |cx| {
            let mut last = cx.factory().nil()?;
            for expr in body {
                last = cx.eval_expr(expr)?;
            }
            Ok(last)
        })
    }
}
