use sim_kernel::{Args, Cx, Expr, NumberLiteral, Result, Symbol, Value};

use crate::{env::LogicEnv, error::logic_eval_error, model::LogicConfig, unify::occurs_check};

// Tower entry points used here:
// - Cx::number_value_ref(&mut self, value: Value) -> Result<Option<NumberValueRef>>
// - Object::as_number_value(&self) -> Option<&dyn NumberValue>
// - sim-lib-numbers-arith exports math/add, math/sub, math/mul, math/div,
//   math/rem, math/pow, and math/cmp as value-level function symbols.
pub(crate) fn eval_is_through_tower(
    cx: &mut Cx,
    config: &LogicConfig,
    left: &Expr,
    right: &Expr,
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let value = eval_arith_term(cx, &env.apply(right))?;
    let result = number_value_to_expr(cx, value)?;
    let mut next = env.clone();
    if next.unify(cx, left, &result, occurs_check(config))? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

pub(crate) fn eval_compare_through_tower(
    cx: &mut Cx,
    op: &Symbol,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [left, right] = args else {
        return Err(logic_eval_error(
            "arithmetic comparison expects two arguments",
        ));
    };
    let left = eval_arith_term(cx, &env.apply(left))?;
    let right = eval_arith_term(cx, &env.apply(right))?;
    let ordering = cx.call_function(
        &Symbol::qualified("math", "cmp"),
        Args::new(vec![left, right]),
    )?;
    let sign = ordering_sign(cx, ordering)?;
    let holds = match op.name.as_ref() {
        "=:=" => sign == 0,
        "=\\=" => sign != 0,
        "<" => sign < 0,
        "=<" => sign <= 0,
        ">" => sign > 0,
        ">=" => sign >= 0,
        other => {
            return Err(logic_eval_error(format!(
                "unknown arithmetic comparison {other}"
            )));
        }
    };
    Ok(if holds { vec![env.clone()] } else { Vec::new() })
}

fn eval_arith_term(cx: &mut Cx, term: &Expr) -> Result<Value> {
    match term {
        Expr::Number(number) => number_literal_value(cx, number),
        Expr::List(items) if items.len() == 3 => {
            let Expr::Symbol(op) = &items[0] else {
                return Err(logic_eval_error("is operator must be a symbol"));
            };
            let left = eval_arith_term(cx, &items[1])?;
            let right = eval_arith_term(cx, &items[2])?;
            eval_operator(cx, op, left, right)
        }
        Expr::Infix {
            operator,
            left,
            right,
        } => {
            let left = eval_arith_term(cx, left)?;
            let right = eval_arith_term(cx, right)?;
            eval_operator(cx, operator, left, right)
        }
        Expr::Local(_) => Err(logic_eval_error("is right-hand side must be ground")),
        _ => Err(logic_eval_error("is term is not arithmetic")),
    }
}

fn number_literal_value(cx: &mut Cx, number: &NumberLiteral) -> Result<Value> {
    let value = cx
        .factory()
        .number_literal(number.domain.clone(), number.canonical.clone())?;
    cx.number_value_ref(value.clone())?
        .map(|_| value)
        .ok_or_else(|| logic_eval_error("is term is not a registered number"))
}

fn number_value_to_expr(cx: &mut Cx, value: Value) -> Result<Expr> {
    let number = cx
        .number_value_ref(value)?
        .ok_or_else(|| logic_eval_error("is result is not a registered number"))?;
    number
        .literal
        .map(Expr::Number)
        .ok_or_else(|| logic_eval_error("is result has no literal form"))
}

fn ordering_sign(cx: &mut Cx, value: Value) -> Result<i8> {
    let number = cx
        .number_value_ref(value)?
        .ok_or_else(|| logic_eval_error("math/cmp result is not a registered number"))?;
    let literal = number
        .literal
        .ok_or_else(|| logic_eval_error("math/cmp result has no literal form"))?;
    let sign = literal
        .canonical
        .parse::<i8>()
        .map_err(|_| logic_eval_error("math/cmp result is not an ordering"))?;
    if matches!(sign, -1..=1) {
        Ok(sign)
    } else {
        Err(logic_eval_error("math/cmp result is out of ordering range"))
    }
}

fn eval_operator(cx: &mut Cx, op: &Symbol, left: Value, right: Value) -> Result<Value> {
    let number_op = match op.name.as_ref() {
        "+" => Symbol::qualified("math", "add"),
        "-" => Symbol::qualified("math", "sub"),
        "*" => Symbol::qualified("math", "mul"),
        "/" => Symbol::qualified("math", "div"),
        "mod" | "rem" | "%" => Symbol::qualified("math", "rem"),
        "**" | "^" => Symbol::qualified("math", "pow"),
        other => {
            return Err(logic_eval_error(format!(
                "is unknown arithmetic operator {other}"
            )));
        }
    };
    cx.call_function(&number_op, Args::new(vec![left, right]))
}
