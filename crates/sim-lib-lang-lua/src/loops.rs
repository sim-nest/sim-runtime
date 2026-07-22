use sim_kernel::{Cx, Error, Expr, Result, Symbol, Value};

use crate::{
    LuaEnv, LuaEvalPolicy, LuaNumber, LuaResult, lua_integer_value, lua_number_from_value,
    lua_table_value,
};

pub(crate) fn eval_numeric_for(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut LuaEnv,
    args: &[Expr],
    eval_one: impl Fn(&LuaEvalPolicy, &mut Cx, &mut LuaEnv, &Expr) -> Result<Value>,
) -> Result<LuaResult> {
    let [name, start, limit, step, body] = args else {
        return Err(Error::Eval(
            "lua numeric for requires name, start, limit, step, and body".to_owned(),
        ));
    };
    let name = binding_symbol(name, "lua numeric for")?;
    let mut index = integer_expr(cx, policy, env, start, &eval_one)?;
    let limit = integer_expr(cx, policy, env, limit, &eval_one)?;
    let step = integer_expr(cx, policy, env, step, &eval_one)?;
    if step == 0 {
        return Err(Error::Eval(
            "lua numeric for step cannot be zero".to_owned(),
        ));
    }

    let mut loop_env = env.child();
    loop_env.define(name.clone(), lua_integer_value(cx, index)?)?;
    let mut last = vec![policy.kit().nil.clone()];
    while if step > 0 {
        index <= limit
    } else {
        index >= limit
    } {
        loop_env.assign(&name, lua_integer_value(cx, index)?)?;
        match policy.eval(cx, &mut loop_env, body)? {
            LuaResult::Values(values) => last = values,
            LuaResult::Return(values) => return Ok(LuaResult::return_values(values)),
            LuaResult::Break => return Ok(LuaResult::values(last)),
        }
        index = index
            .checked_add(step)
            .ok_or_else(|| Error::Eval("lua numeric for index overflow".to_owned()))?;
    }
    Ok(LuaResult::values(last))
}

pub(crate) fn eval_generic_for(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut LuaEnv,
    args: &[Expr],
    eval_one: impl Fn(&LuaEvalPolicy, &mut Cx, &mut LuaEnv, &Expr) -> Result<Value>,
) -> Result<LuaResult> {
    let [key_name, value_name, table_expr, body] = args else {
        return Err(Error::Eval(
            "lua generic for requires key name, value name, table, and body".to_owned(),
        ));
    };
    let key_name = binding_symbol(key_name, "lua generic for")?;
    let value_name = binding_symbol(value_name, "lua generic for")?;
    let table_value = eval_one(policy, cx, env, table_expr)?;
    let entries = lua_table_value(&table_value)?.entries_in_key_order()?;

    let mut loop_env = env.child();
    loop_env.define(key_name.clone(), policy.kit().nil.clone())?;
    loop_env.define(value_name.clone(), policy.kit().nil.clone())?;
    let mut last = vec![policy.kit().nil.clone()];
    for (key, value) in entries {
        loop_env.assign(&key_name, value_from_expr(cx, key.as_expr())?)?;
        loop_env.assign(&value_name, value)?;
        match policy.eval(cx, &mut loop_env, body)? {
            LuaResult::Values(values) => last = values,
            LuaResult::Return(values) => return Ok(LuaResult::return_values(values)),
            LuaResult::Break => return Ok(LuaResult::values(last)),
        }
    }
    Ok(LuaResult::values(last))
}

fn integer_expr(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut LuaEnv,
    expr: &Expr,
    eval_one: &impl Fn(&LuaEvalPolicy, &mut Cx, &mut LuaEnv, &Expr) -> Result<Value>,
) -> Result<i64> {
    let value = eval_one(policy, cx, env, expr)?;
    match lua_number_from_value(cx, &value)? {
        Some(LuaNumber::Integer(value)) => Ok(value),
        Some(LuaNumber::Float(value)) if value.fract() == 0.0 => Ok(value as i64),
        _ => Err(Error::Eval(
            "lua numeric for bound must be an integer".to_owned(),
        )),
    }
}

fn binding_symbol(expr: &Expr, context: &str) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) | Expr::Local(symbol) => Ok(symbol.clone()),
        _ => Err(Error::Eval(format!(
            "{context} requires a symbol binding target"
        ))),
    }
}

fn value_from_expr(cx: &mut Cx, expr: Expr) -> Result<Value> {
    match expr {
        Expr::Nil => cx.factory().nil(),
        Expr::Bool(value) => cx.factory().bool(value),
        Expr::Number(number) => cx.factory().number_literal(number.domain, number.canonical),
        Expr::String(value) => cx.factory().string(value),
        Expr::Symbol(symbol) => cx.factory().symbol(symbol),
        other => cx.factory().expr(other),
    }
}
