use sim_kernel::{Cx, Datum, DatumStore, Expr, Ref, Result, Symbol, logic_tool_call_capability};
use sim_lib_control::{ControlPrompt, ControlTag, raise_prompt};

use crate::{env::LogicEnv, error::logic_eval_error, model::LogicConfig, unify::occurs_check};

pub(crate) fn solve_constraint(
    cx: &mut Cx,
    config: &LogicConfig,
    key: &Symbol,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    match key.name.as_ref() {
        "#=" | "#<" | "dif" => constraint_through_ledger(cx, key, args, env),
        "=" => solve_eq(config, args, env),
        "<" => solve_compare("<", args, env),
        "<=" => solve_compare("<=", args, env),
        ">" => solve_compare(">", args, env),
        ">=" => solve_compare(">=", args, env),
        "plus" => solve_arith(cx, config, "plus", args, env),
        "minus" => solve_arith(cx, config, "minus", args, env),
        "times" => solve_arith(cx, config, "times", args, env),
        "between" => solve_between(config, args, env),
        "tool-call" => solve_tool_call(cx, config, args, env),
        _ => Ok(Vec::new()),
    }
}

/// Constraint demand posted as the input of a control-prompt effect.
#[derive(Clone, Debug)]
pub(crate) struct ConstraintDemand {
    relation: Symbol,
    args: Vec<Expr>,
    input: Ref,
}

impl ConstraintDemand {
    fn new(cx: &mut Cx, relation: Symbol, args: Vec<Expr>) -> Result<Self> {
        let input = intern_constraint_demand(cx, &relation, &args)?;
        Ok(Self {
            relation,
            args,
            input,
        })
    }
}

impl ControlPrompt for ConstraintDemand {
    fn tag(&self) -> ControlTag {
        ControlTag::new(Symbol::qualified("logic", "constraint"))
    }

    fn input(&self) -> Ref {
        self.input.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConstraintVerdict {
    Entailed,
    Disentailed,
    Residual,
}

fn constraint_through_ledger(
    cx: &mut Cx,
    relation: &Symbol,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let applied = args.iter().map(|arg| env.apply(arg)).collect::<Vec<_>>();
    let demand = ConstraintDemand::new(cx, relation.clone(), applied)?;
    let _posted = raise_prompt(cx, &demand)?;
    match constraint_verdict(&demand)? {
        ConstraintVerdict::Entailed => Ok(vec![env.clone()]),
        ConstraintVerdict::Disentailed => Ok(Vec::new()),
        ConstraintVerdict::Residual => Err(logic_eval_error(format!(
            "residual constraint demand suspended on ledger: {}",
            demand.relation
        ))),
    }
}

fn constraint_verdict(demand: &ConstraintDemand) -> Result<ConstraintVerdict> {
    let [left, right] = demand.args.as_slice() else {
        return Err(logic_eval_error(format!(
            "{} expects two arguments",
            demand.relation
        )));
    };
    if contains_local(left) || contains_local(right) {
        return Ok(ConstraintVerdict::Residual);
    }
    match demand.relation.name.as_ref() {
        "#=" => Ok(if left.canonical_eq(right) {
            ConstraintVerdict::Entailed
        } else {
            ConstraintVerdict::Disentailed
        }),
        "#<" => Ok(if eval_number(left)? < eval_number(right)? {
            ConstraintVerdict::Entailed
        } else {
            ConstraintVerdict::Disentailed
        }),
        "dif" => Ok(if left.canonical_eq(right) {
            ConstraintVerdict::Disentailed
        } else {
            ConstraintVerdict::Entailed
        }),
        other => Err(logic_eval_error(format!(
            "unsupported constraint relation {other}"
        ))),
    }
}

fn solve_eq(config: &LogicConfig, args: &[Expr], env: &LogicEnv) -> Result<Vec<LogicEnv>> {
    let [left, right] = args else {
        return Err(logic_eval_error("= expects two arguments"));
    };
    let mut next = env.clone();
    if next.unify(left, right, occurs_check(config))? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

fn solve_compare(op: &str, args: &[Expr], env: &LogicEnv) -> Result<Vec<LogicEnv>> {
    let [left, right] = args else {
        return Err(logic_eval_error("comparison expects two arguments"));
    };
    let left = eval_number(&env.apply(left))?;
    let right = eval_number(&env.apply(right))?;
    let accepted = match op {
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => false,
    };
    if accepted {
        Ok(vec![env.clone()])
    } else {
        Ok(Vec::new())
    }
}

fn solve_arith(
    cx: &mut Cx,
    config: &LogicConfig,
    op: &str,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [left, right, out] = args else {
        return Err(logic_eval_error(format!("{op} expects three arguments")));
    };
    let left_applied = env.apply(left);
    let right_applied = env.apply(right);
    let out_applied = env.apply(out);
    let unknowns = [
        left_applied.clone(),
        right_applied.clone(),
        out_applied.clone(),
    ]
    .iter()
    .filter(|expr| matches!(expr, Expr::Local(_)))
    .count();
    if unknowns > 1 {
        return Err(logic_eval_error(format!(
            "unbounded numeric constraint {op} requires finite domains"
        )));
    }

    let mut next = env.clone();
    match (left_applied, right_applied, out_applied) {
        (Expr::Local(var), known_right, known_out) => {
            let right = eval_number(&known_right)?;
            let out = eval_number(&known_out)?;
            let value = match op {
                "plus" => out - right,
                "minus" => out + right,
                "times" => out / right,
                _ => return Err(logic_eval_error(format!("unsupported op {op}"))),
            };
            next.bind(var, number_expr(cx, value), occurs_check(config))?;
        }
        (known_left, Expr::Local(var), known_out) => {
            let left = eval_number(&known_left)?;
            let out = eval_number(&known_out)?;
            let value = match op {
                "plus" => out - left,
                "minus" => left - out,
                "times" => out / left,
                _ => return Err(logic_eval_error(format!("unsupported op {op}"))),
            };
            next.bind(var, number_expr(cx, value), occurs_check(config))?;
        }
        (known_left, known_right, Expr::Local(var)) => {
            let left = eval_number(&known_left)?;
            let right = eval_number(&known_right)?;
            let value = match op {
                "plus" => left + right,
                "minus" => left - right,
                "times" => left * right,
                _ => return Err(logic_eval_error(format!("unsupported op {op}"))),
            };
            next.bind(var, number_expr(cx, value), occurs_check(config))?;
        }
        (known_left, known_right, known_out) => {
            let left = eval_number(&known_left)?;
            let right = eval_number(&known_right)?;
            let out = eval_number(&known_out)?;
            let accepted = match op {
                "plus" => (left + right - out).abs() < f64::EPSILON,
                "minus" => (left - right - out).abs() < f64::EPSILON,
                "times" => (left * right - out).abs() < f64::EPSILON,
                _ => false,
            };
            if !accepted {
                return Ok(Vec::new());
            }
        }
    }

    Ok(vec![next])
}

fn solve_between(config: &LogicConfig, args: &[Expr], env: &LogicEnv) -> Result<Vec<LogicEnv>> {
    let [min_expr, max_expr, out_expr] = args else {
        return Err(logic_eval_error("between expects three arguments"));
    };
    let min = eval_integer(&env.apply(min_expr))?;
    let max = eval_integer(&env.apply(max_expr))?;
    if min > max {
        return Ok(Vec::new());
    }
    let applied_out = env.apply(out_expr);
    match applied_out {
        Expr::Local(var) => {
            let mut answers = Vec::new();
            for value in min..=max {
                let mut next = env.clone();
                next.bind(
                    var.clone(),
                    Expr::Number(sim_kernel::NumberLiteral {
                        domain: Symbol::qualified("numbers", "i64"),
                        canonical: value.to_string(),
                    }),
                    occurs_check(config),
                )?;
                answers.push(next);
            }
            Ok(answers)
        }
        other => {
            let value = eval_integer(&other)?;
            if (min..=max).contains(&value) {
                Ok(vec![env.clone()])
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn solve_tool_call(
    cx: &mut Cx,
    config: &LogicConfig,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    cx.require(&logic_tool_call_capability())?;
    let [tool_expr, args_expr, result_expr] = args else {
        return Err(logic_eval_error("tool-call expects tool, args, and result"));
    };

    let tool_value = match env.apply(tool_expr) {
        Expr::Symbol(symbol) => cx.eval_expr(Expr::Symbol(symbol))?,
        other => {
            return Err(logic_eval_error(format!(
                "tool-call tool must resolve from a symbol, found {other:?}"
            )));
        }
    };

    let tool_args = match env.apply(args_expr) {
        Expr::List(items) | Expr::Vector(items) => items,
        other => {
            return Err(logic_eval_error(format!(
                "tool-call args must be a list or vector, found {other:?}"
            )));
        }
    };

    let result = cx.call_exprs(tool_value, tool_args)?;
    let result = result.object().as_expr(cx)?;
    let mut next = env.clone();
    if next.unify(result_expr, &result, occurs_check(config))? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

fn eval_number(expr: &Expr) -> Result<f64> {
    match expr {
        Expr::Number(number) => number.canonical.parse::<f64>().map_err(|_| {
            logic_eval_error(format!(
                "numeric constraint only supports scalar literals, found {}",
                number.canonical
            ))
        }),
        _ => Err(logic_eval_error(
            "numeric constraint expected a number literal",
        )),
    }
}

fn eval_integer(expr: &Expr) -> Result<i64> {
    match expr {
        Expr::Number(number) => number.canonical.parse::<i64>().map_err(|_| {
            logic_eval_error(format!(
                "bounded numeric generation expects an integer literal, found {}",
                number.canonical
            ))
        }),
        _ => Err(logic_eval_error(
            "bounded numeric generation expected an integer literal",
        )),
    }
}

fn number_expr(cx: &mut Cx, value: f64) -> Expr {
    let _ = cx;
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

fn intern_constraint_demand(cx: &mut Cx, relation: &Symbol, args: &[Expr]) -> Result<Ref> {
    let args = args
        .iter()
        .map(expr_datum_with_locals)
        .collect::<Result<Vec<_>>>()?;
    let id = cx.datum_store_mut().intern(Datum::Node {
        tag: Symbol::qualified("logic", "ConstraintDemand"),
        fields: vec![
            (Symbol::new("relation"), Datum::Symbol(relation.clone())),
            (Symbol::new("args"), Datum::List(args)),
        ],
    })?;
    Ok(Ref::Content(id))
}

fn expr_datum_with_locals(expr: &Expr) -> Result<Datum> {
    match expr {
        Expr::Local(symbol) => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Local"),
            fields: vec![(Symbol::new("name"), Datum::Symbol(symbol.clone()))],
        }),
        Expr::List(items) => items
            .iter()
            .map(expr_datum_with_locals)
            .collect::<Result<Vec<_>>>()
            .map(Datum::List),
        Expr::Vector(items) => items
            .iter()
            .map(expr_datum_with_locals)
            .collect::<Result<Vec<_>>>()
            .map(Datum::Vector),
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| Ok((expr_datum_with_locals(key)?, expr_datum_with_locals(value)?)))
            .collect::<Result<Vec<_>>>()
            .map(Datum::Map),
        Expr::Set(items) => items
            .iter()
            .map(expr_datum_with_locals)
            .collect::<Result<Vec<_>>>()
            .map(Datum::Set),
        Expr::Block(items) => items
            .iter()
            .map(expr_datum_with_locals)
            .collect::<Result<Vec<_>>>()
            .map(|items| Datum::Node {
                tag: Symbol::qualified("logic", "Block"),
                fields: vec![(Symbol::new("items"), Datum::List(items))],
            }),
        Expr::Call { operator, args } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Call"),
            fields: vec![
                (Symbol::new("operator"), expr_datum_with_locals(operator)?),
                (
                    Symbol::new("args"),
                    Datum::List(
                        args.iter()
                            .map(expr_datum_with_locals)
                            .collect::<Result<Vec<_>>>()?,
                    ),
                ),
            ],
        }),
        Expr::Infix {
            operator,
            left,
            right,
        } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Infix"),
            fields: vec![
                (Symbol::new("operator"), Datum::Symbol(operator.clone())),
                (Symbol::new("left"), expr_datum_with_locals(left)?),
                (Symbol::new("right"), expr_datum_with_locals(right)?),
            ],
        }),
        Expr::Prefix { operator, arg } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Prefix"),
            fields: vec![
                (Symbol::new("operator"), Datum::Symbol(operator.clone())),
                (Symbol::new("arg"), expr_datum_with_locals(arg)?),
            ],
        }),
        Expr::Postfix { operator, arg } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Postfix"),
            fields: vec![
                (Symbol::new("operator"), Datum::Symbol(operator.clone())),
                (Symbol::new("arg"), expr_datum_with_locals(arg)?),
            ],
        }),
        Expr::Quote { mode, expr } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Quote"),
            fields: vec![
                (
                    Symbol::new("mode"),
                    Datum::Symbol(Symbol::new(format!("{mode:?}"))),
                ),
                (Symbol::new("expr"), expr_datum_with_locals(expr)?),
            ],
        }),
        Expr::Annotated { expr, annotations } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Annotated"),
            fields: vec![
                (Symbol::new("expr"), expr_datum_with_locals(expr)?),
                (
                    Symbol::new("annotations"),
                    Datum::List(
                        annotations
                            .iter()
                            .map(|(name, value)| {
                                Ok(Datum::Node {
                                    tag: Symbol::qualified("logic", "Annotation"),
                                    fields: vec![
                                        (Symbol::new("name"), Datum::Symbol(name.clone())),
                                        (Symbol::new("value"), expr_datum_with_locals(value)?),
                                    ],
                                })
                            })
                            .collect::<Result<Vec<_>>>()?,
                    ),
                ),
            ],
        }),
        Expr::Extension { tag, payload } => Ok(Datum::Node {
            tag: Symbol::qualified("logic", "Extension"),
            fields: vec![
                (Symbol::new("tag"), Datum::Symbol(tag.clone())),
                (Symbol::new("payload"), expr_datum_with_locals(payload)?),
            ],
        }),
        other => Datum::try_from(other.clone()),
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
