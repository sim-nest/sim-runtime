//! Clause model: the facts and rules that make up a logic program.
//!
//! A [`Clause`] is the parsed form of a `(fact ...)` or `(rule head body)`
//! surface expression; [`parse_clause_expr`] performs that parse. Clauses are
//! the concrete behavior this organ adds on top of the kernel `Expr` graph; see
//! the [`README`](https://docs.rs/sim-runtime).

use std::collections::BTreeMap;

use sim_kernel::{Expr, Origin, Result, Symbol};

use crate::error::{ensure, logic_eval_error};

/// Stable identifier assigned to a clause when it is added to a database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClauseId(pub usize);

/// A parsed logic clause: a head goal plus an optional list of body goals.
///
/// A clause with an empty `body` is a fact; one with body goals is a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    /// Identifier of this clause within its database.
    pub id: ClauseId,
    /// The head goal (the conclusion the clause proves).
    pub head: Expr,
    /// Body goals that must all hold for the head to hold; empty for a fact.
    pub body: Vec<Expr>,
    /// Source origin of the clause, when known.
    pub source: Option<Origin>,
}

impl Clause {
    /// Returns the predicate symbol of the clause head.
    pub fn predicate(&self) -> Result<Symbol> {
        predicate_symbol(&self.head)
    }

    /// Returns the arity (argument count) of the clause head.
    pub fn arity(&self) -> Result<usize> {
        goal_arity(&self.head)
    }

    /// Renders the clause back to its canonical `(fact ...)` or `(rule ...)`
    /// surface expression.
    pub fn fact_expr(&self) -> Expr {
        if self.body.is_empty() {
            Expr::List(vec![
                Expr::Symbol(Symbol::new("fact")),
                normalize_goal_expr(&self.head),
            ])
        } else {
            Expr::List(vec![
                Expr::Symbol(Symbol::new("rule")),
                normalize_goal_expr(&self.head),
                Expr::List(self.body.iter().map(normalize_goal_expr).collect()),
            ])
        }
    }
}

/// Parses a `(fact head)` or `(rule head body)` expression into a [`Clause`].
///
/// The expression must be a list whose first element is the symbol `fact` or
/// `rule`; any other form is rejected with a logic eval error.
///
/// # Examples
///
/// ```
/// use sim_kernel::{Expr, Symbol};
/// use sim_lib_logic::{ClauseId, parse_clause_expr};
///
/// let fact = Expr::List(vec![
///     Expr::Symbol(Symbol::new("fact")),
///     Expr::List(vec![
///         Expr::Symbol(Symbol::new("parent")),
///         Expr::Symbol(Symbol::new("alice")),
///         Expr::Symbol(Symbol::new("bob")),
///     ]),
/// ]);
/// let clause = parse_clause_expr(ClauseId(1), fact).unwrap();
/// assert!(clause.body.is_empty());
/// assert_eq!(clause.predicate().unwrap(), Symbol::new("parent"));
/// assert_eq!(clause.arity().unwrap(), 2);
/// ```
pub fn parse_clause_expr(id: ClauseId, expr: Expr) -> Result<Clause> {
    let Expr::List(items) = expr else {
        return Err(logic_eval_error("logic clause must be a list"));
    };
    let Some((head, tail)) = items.split_first() else {
        return Err(logic_eval_error("logic clause cannot be empty"));
    };
    match head {
        Expr::Symbol(symbol) if symbol.name.as_ref() == "fact" => parse_fact(id, tail),
        Expr::Symbol(symbol) if symbol.name.as_ref() == "rule" => parse_rule(id, tail),
        _ => Err(logic_eval_error(
            "logic clause must start with fact or rule",
        )),
    }
}

pub fn parse_goal_expr(expr: &Expr) -> Result<Expr> {
    ensure(is_goal_expr(expr), "logic goal must be a call-shaped list")?;
    Ok(normalize_goal_expr(expr))
}

pub(crate) fn is_cut_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Symbol(symbol) if symbol.namespace.is_none() && symbol.name.as_ref() == "!")
}

pub fn is_goal_expr(expr: &Expr) -> bool {
    match expr {
        Expr::List(items) => matches!(items.first(), Some(Expr::Symbol(_))),
        Expr::Call { operator, .. } => matches!(operator.as_ref(), Expr::Symbol(_)),
        _ => false,
    }
}

pub fn predicate_symbol(expr: &Expr) -> Result<Symbol> {
    match expr {
        Expr::List(items) => match items.first() {
            Some(Expr::Symbol(symbol)) => Ok(symbol.clone()),
            _ => Err(logic_eval_error("goal head operator must be a symbol")),
        },
        Expr::Call { operator, .. } => match operator.as_ref() {
            Expr::Symbol(symbol) => Ok(symbol.clone()),
            _ => Err(logic_eval_error("goal head operator must be a symbol")),
        },
        _ => Err(logic_eval_error("goal must be call-shaped")),
    }
}

pub fn goal_arity(expr: &Expr) -> Result<usize> {
    match expr {
        Expr::List(items) => Ok(items.len().saturating_sub(1)),
        Expr::Call { args, .. } => Ok(args.len()),
        _ => Err(logic_eval_error("goal must be call-shaped")),
    }
}

pub fn goal_first_arg(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::List(items) => items.get(1),
        Expr::Call { args, .. } => args.first(),
        _ => None,
    }
}

pub fn normalize_goal_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Call { operator, args } => {
            let mut items = Vec::with_capacity(args.len() + 1);
            items.push((**operator).clone());
            items.extend(args.iter().cloned());
            Expr::List(items)
        }
        other => other.clone(),
    }
}

pub fn rename_clause_apart(clause: &Clause, depth: usize) -> Clause {
    let mut renamed = BTreeMap::new();
    let mut anonymous_index = 0usize;
    let suffix = format!("{}:{}", clause.id.0, depth);
    Clause {
        id: clause.id,
        head: rename_expr(&clause.head, &suffix, &mut renamed, &mut anonymous_index),
        body: clause
            .body
            .iter()
            .map(|goal| rename_expr(goal, &suffix, &mut renamed, &mut anonymous_index))
            .collect(),
        source: clause.source.clone(),
    }
}

fn parse_fact(id: ClauseId, tail: &[Expr]) -> Result<Clause> {
    let [head] = tail else {
        return Err(logic_eval_error("fact expects one call head"));
    };
    let head = parse_goal_expr(head)?;
    Ok(Clause {
        id,
        head,
        body: Vec::new(),
        source: None,
    })
}

fn parse_rule(id: ClauseId, tail: &[Expr]) -> Result<Clause> {
    let [head, body] = tail else {
        return Err(logic_eval_error("rule expects head plus body list"));
    };
    let head = parse_goal_expr(head)?;
    let Expr::List(goals) = body else {
        return Err(logic_eval_error("rule body must be a list of goals"));
    };
    let body = goals
        .iter()
        .map(parse_rule_body_goal)
        .collect::<Result<Vec<_>>>()?;
    Ok(Clause {
        id,
        head,
        body,
        source: None,
    })
}

fn parse_rule_body_goal(expr: &Expr) -> Result<Expr> {
    if is_cut_expr(expr) {
        return Ok(expr.clone());
    }
    parse_goal_expr(expr)
}

fn rename_expr(
    expr: &Expr,
    suffix: &str,
    renamed: &mut BTreeMap<Symbol, Symbol>,
    anonymous_index: &mut usize,
) -> Expr {
    match expr {
        Expr::Local(var) if var.name.as_ref() == "_" => {
            *anonymous_index += 1;
            Expr::Local(Symbol::new(format!("__anon_{suffix}_{anonymous_index}")))
        }
        Expr::Local(var) => {
            let name = renamed
                .entry(var.clone())
                .or_insert_with(|| Symbol::new(format!("{}@{suffix}", var.name)))
                .clone();
            Expr::Local(name)
        }
        Expr::List(items) => Expr::List(
            items
                .iter()
                .map(|item| rename_expr(item, suffix, renamed, anonymous_index))
                .collect(),
        ),
        Expr::Vector(items) => Expr::Vector(
            items
                .iter()
                .map(|item| rename_expr(item, suffix, renamed, anonymous_index))
                .collect(),
        ),
        Expr::Map(entries) => Expr::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        rename_expr(key, suffix, renamed, anonymous_index),
                        rename_expr(value, suffix, renamed, anonymous_index),
                    )
                })
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(
            items
                .iter()
                .map(|item| rename_expr(item, suffix, renamed, anonymous_index))
                .collect(),
        ),
        Expr::Call { operator, args } => Expr::Call {
            operator: Box::new(rename_expr(operator, suffix, renamed, anonymous_index)),
            args: args
                .iter()
                .map(|arg| rename_expr(arg, suffix, renamed, anonymous_index))
                .collect(),
        },
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Infix {
            operator: operator.clone(),
            left: Box::new(rename_expr(left, suffix, renamed, anonymous_index)),
            right: Box::new(rename_expr(right, suffix, renamed, anonymous_index)),
        },
        Expr::Prefix { operator, arg } => Expr::Prefix {
            operator: operator.clone(),
            arg: Box::new(rename_expr(arg, suffix, renamed, anonymous_index)),
        },
        Expr::Postfix { operator, arg } => Expr::Postfix {
            operator: operator.clone(),
            arg: Box::new(rename_expr(arg, suffix, renamed, anonymous_index)),
        },
        Expr::Block(items) => Expr::Block(
            items
                .iter()
                .map(|item| rename_expr(item, suffix, renamed, anonymous_index))
                .collect(),
        ),
        Expr::Quote { mode, expr } => Expr::Quote {
            mode: *mode,
            expr: Box::new(rename_expr(expr, suffix, renamed, anonymous_index)),
        },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(rename_expr(expr, suffix, renamed, anonymous_index)),
            annotations: annotations
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        rename_expr(value, suffix, renamed, anonymous_index),
                    )
                })
                .collect(),
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag: tag.clone(),
            payload: Box::new(rename_expr(payload, suffix, renamed, anonymous_index)),
        },
        other => other.clone(),
    }
}
