use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::SharedOrganRuntime;

use crate::{
    LuaEvalPolicy, LuaNumber, lua_core_profile, lua_float_value, lua_get_metatable,
    lua_integer_value, lua_number_from_value, lua_rawget, lua_rawset, lua_set_metatable,
    lua_table_from_values, lua_table_value,
};

use crate::call::{call_lua_value, protected_lua_call};

#[derive(Clone, Copy)]
pub(crate) enum LuaBaseKind {
    PCall,
    XPCall,
    Assert,
    Error,
    Select,
    Type,
    ToString,
    ToNumber,
    Pairs,
    IPairs,
    Next,
    RawGet,
    RawSet,
    RawEqual,
    RawLen,
    SetMetatable,
    GetMetatable,
    Print,
}

impl LuaBaseKind {
    const ALL: [Self; 18] = [
        Self::PCall,
        Self::XPCall,
        Self::Assert,
        Self::Error,
        Self::Select,
        Self::Type,
        Self::ToString,
        Self::ToNumber,
        Self::Pairs,
        Self::IPairs,
        Self::Next,
        Self::RawGet,
        Self::RawSet,
        Self::RawEqual,
        Self::RawLen,
        Self::SetMetatable,
        Self::GetMetatable,
        Self::Print,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::PCall => "pcall",
            Self::XPCall => "xpcall",
            Self::Assert => "assert",
            Self::Error => "error",
            Self::Select => "select",
            Self::Type => "type",
            Self::ToString => "tostring",
            Self::ToNumber => "tonumber",
            Self::Pairs => "pairs",
            Self::IPairs => "ipairs",
            Self::Next => "next",
            Self::RawGet => "rawget",
            Self::RawSet => "rawset",
            Self::RawEqual => "rawequal",
            Self::RawLen => "rawlen",
            Self::SetMetatable => "setmetatable",
            Self::GetMetatable => "getmetatable",
            Self::Print => "print",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/base", self.env_name())
    }

    fn organ(self) -> Symbol {
        match self {
            Self::PCall | Self::XPCall | Self::Error => sim_lib_control::control_organ_symbol(),
            Self::Pairs | Self::IPairs | Self::Next => sim_lib_sequence::sequence_organ_symbol(),
            Self::RawGet
            | Self::RawSet
            | Self::RawEqual
            | Self::RawLen
            | Self::SetMetatable
            | Self::GetMetatable => sim_lib_mutation::mutation_organ_symbol(),
            Self::Assert
            | Self::Select
            | Self::Type
            | Self::ToString
            | Self::ToNumber
            | Self::Print => sim_lib_dispatch::dispatch_organ_symbol(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct LuaBaseFunction {
    kind: LuaBaseKind,
}

impl LuaBaseFunction {
    fn new(kind: LuaBaseKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaBaseKind {
        self.kind
    }
}

impl Object for LuaBaseFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-base-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaBaseFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaBaseFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_base_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, sim_lib_standard_core::Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_base_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    for kind in LuaBaseKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaBaseFunction::new(kind)))?;
        runtime.define_function(
            &profile_symbol,
            kind.organ(),
            kind.function_symbol(),
            function.clone(),
        )?;
        define_or_assign(env, Symbol::new(kind.env_name()), function)?;
    }

    define_or_assign(
        env,
        Symbol::new("_VERSION"),
        cx.factory().string("Lua 5.4".to_owned())?,
    )?;
    Ok(())
}

pub(crate) fn run_lua_base_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaBaseKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaBaseKind::PCall => lua_pcall(cx, policy, args),
        LuaBaseKind::XPCall => lua_xpcall(cx, policy, args),
        LuaBaseKind::Assert => lua_assert(cx, policy, args),
        LuaBaseKind::Error => lua_error(cx, args),
        LuaBaseKind::Select => lua_select(cx, args),
        LuaBaseKind::Type => lua_type(cx, args),
        LuaBaseKind::ToString => lua_tostring(cx, args),
        LuaBaseKind::ToNumber => lua_tonumber(cx, args),
        LuaBaseKind::Pairs => {
            table_pairs_list(cx, &first_arg(args, "pairs")?, false).map(|value| vec![value])
        }
        LuaBaseKind::IPairs => {
            table_pairs_list(cx, &first_arg(args, "ipairs")?, true).map(|value| vec![value])
        }
        LuaBaseKind::Next => lua_next(cx, args),
        LuaBaseKind::RawGet => lua_raw_get_function(cx, policy, args),
        LuaBaseKind::RawSet => lua_raw_set_function(cx, args),
        LuaBaseKind::RawEqual => lua_raw_equal(cx, args),
        LuaBaseKind::RawLen => lua_raw_len(cx, args),
        LuaBaseKind::SetMetatable => lua_set_metatable_function(cx, args),
        LuaBaseKind::GetMetatable => lua_get_metatable_function(cx, policy, args),
        LuaBaseKind::Print => lua_print(cx, policy, args),
    }
}

fn lua_pcall(cx: &mut Cx, policy: &LuaEvalPolicy, mut args: Vec<Value>) -> Result<Vec<Value>> {
    let function = required_arg(&mut args, "pcall")?;
    match protected_lua_call(cx, policy, function, args)? {
        sim_lib_control::ProtectedOutcome::Returned(values) => {
            let mut out = vec![cx.factory().bool(true)?];
            out.extend(values);
            Ok(out)
        }
        sim_lib_control::ProtectedOutcome::Raised(value) => {
            Ok(vec![cx.factory().bool(false)?, value])
        }
    }
}

fn lua_xpcall(cx: &mut Cx, policy: &LuaEvalPolicy, mut args: Vec<Value>) -> Result<Vec<Value>> {
    let function = required_arg(&mut args, "xpcall")?;
    let handler = required_arg(&mut args, "xpcall")?;
    match protected_lua_call(cx, policy, function, args)? {
        sim_lib_control::ProtectedOutcome::Returned(values) => {
            let mut out = vec![cx.factory().bool(true)?];
            out.extend(values);
            Ok(out)
        }
        sim_lib_control::ProtectedOutcome::Raised(value) => {
            let handled = call_lua_value(cx, policy, handler, vec![value])?;
            let mut out = vec![cx.factory().bool(false)?];
            out.extend(handled);
            Ok(out)
        }
    }
}

fn lua_assert(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let Some(first) = args.first() else {
        return Err(Error::Eval("assert requires a value".to_owned()));
    };
    if policy.kit().is_truthy(cx, first)? {
        return Ok(args);
    }
    let message = args
        .get(1)
        .map(|value| value.object().display(cx))
        .transpose()?
        .unwrap_or_else(|| "assertion failed!".to_owned());
    Err(Error::Eval(message))
}

fn lua_error(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let message = args
        .first()
        .map(|value| value.object().display(cx))
        .transpose()?
        .unwrap_or_else(|| "lua error".to_owned());
    Err(Error::Eval(message))
}

fn lua_select(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let Some(selector) = args.first() else {
        return Err(Error::Eval("select requires a selector".to_owned()));
    };
    let values = &args[1..];
    match selector.object().as_expr(cx)? {
        Expr::String(text) if text == "#" => {
            lua_integer_value(cx, values.len() as i64).map(|v| vec![v])
        }
        Expr::Number(number) => {
            let index = number
                .canonical
                .parse::<isize>()
                .map_err(|_| Error::Eval("select index must be an integer".to_owned()))?;
            let start = if index < 0 {
                values.len() as isize + index
            } else {
                index - 1
            };
            if start < 0 {
                return Err(Error::Eval("select index out of range".to_owned()));
            }
            Ok(values.iter().skip(start as usize).cloned().collect())
        }
        _ => Err(Error::Eval(
            "select selector must be '#' or an integer".to_owned(),
        )),
    }
}

fn lua_type(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = args
        .first()
        .cloned()
        .unwrap_or_else(|| cx.factory().nil().unwrap());
    let name = if value
        .object()
        .downcast_ref::<crate::stdlib_coroutine::LuaThread>()
        .is_some()
    {
        "thread"
    } else if lua_table_value(&value).is_ok() {
        "table"
    } else if value.object().as_callable().is_some() {
        "function"
    } else {
        match value.object().as_expr(cx)? {
            Expr::Nil => "nil",
            Expr::Bool(_) => "boolean",
            Expr::Number(_) => "number",
            Expr::String(_) => "string",
            _ => "userdata",
        }
    };
    cx.factory()
        .string(name.to_owned())
        .map(|value| vec![value])
}

fn lua_tostring(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = args
        .first()
        .cloned()
        .unwrap_or_else(|| cx.factory().nil().unwrap());
    let display = value.object().display(cx)?;
    cx.factory().string(display).map(|value| vec![value])
}

fn lua_tonumber(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let Some(value) = args.first() else {
        return Ok(vec![cx.factory().nil()?]);
    };
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => lua_integer_value(cx, value).map(|value| vec![value]),
        Some(LuaNumber::Float(value)) => lua_float_value(cx, value).map(|value| vec![value]),
        None => Ok(vec![cx.factory().nil()?]),
    }
}

fn lua_next(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let table_value = args
        .first()
        .ok_or_else(|| Error::Eval("next requires a table".to_owned()))?;
    let after_key = args.get(1);
    let table = lua_table_value(table_value)?;
    let mut found_after = after_key
        .map(|key| matches!(key.object().as_expr(cx), Ok(Expr::Nil)))
        .unwrap_or(true);
    for (key, value) in table.entries_in_key_order()? {
        let key_value = value_from_expr(cx, key.as_expr())?;
        if found_after {
            return Ok(vec![key_value, value]);
        }
        if let Some(after_key) = after_key {
            found_after = key_value.object().as_expr(cx)? == after_key.object().as_expr(cx)?;
        }
    }
    Ok(vec![cx.factory().nil()?])
}

fn lua_raw_get_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let [table, key] = args.as_slice() else {
        return Err(Error::Eval("rawget requires table and key".to_owned()));
    };
    Ok(vec![
        lua_rawget(cx, table, key)?.unwrap_or_else(|| policy.kit().nil.clone()),
    ])
}

fn lua_raw_set_function(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let [table, key, value] = args.as_slice() else {
        return Err(Error::Eval(
            "rawset requires table, key, and value".to_owned(),
        ));
    };
    lua_rawset(cx, table, key.clone(), value.clone())?;
    Ok(vec![table.clone()])
}

fn lua_raw_equal(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let [left, right] = args.as_slice() else {
        return Err(Error::Eval("rawequal requires two values".to_owned()));
    };
    cx.factory().bool(left == right).map(|value| vec![value])
}

fn lua_raw_len(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = first_arg(args, "rawlen")?;
    match value.object().as_expr(cx)? {
        Expr::String(text) => lua_integer_value(cx, text.chars().count() as i64).map(|v| vec![v]),
        _ => {
            let len = lua_table_value(&value)?.len_border(cx)?;
            lua_integer_value(cx, len).map(|v| vec![v])
        }
    }
}

fn lua_set_metatable_function(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let [table, metatable] = args.as_slice() else {
        return Err(Error::Eval(
            "setmetatable requires table and metatable".to_owned(),
        ));
    };
    lua_set_metatable(cx, table, metatable.clone())?;
    Ok(vec![table.clone()])
}

fn lua_get_metatable_function(
    _cx: &mut Cx,
    policy: &LuaEvalPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let table = first_arg(args, "getmetatable")?;
    Ok(vec![
        lua_get_metatable(&table)?.unwrap_or_else(|| policy.kit().nil.clone()),
    ])
}

fn lua_print(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let mut rendered = Vec::with_capacity(args.len());
    for value in args {
        rendered.push(value.object().display(cx)?);
    }
    if !rendered.is_empty() {
        println!("{}", rendered.join("\t"));
    }
    Ok(vec![policy.kit().nil.clone()])
}

fn table_pairs_list(cx: &mut Cx, table_value: &Value, array_only: bool) -> Result<Value> {
    let table = lua_table_value(table_value)?;
    let mut rows = Vec::new();
    for (key, value) in table.entries_in_key_order()? {
        if array_only && key.as_integer_index().is_none() {
            continue;
        }
        let first_key = lua_integer_value(cx, 1)?;
        let row_key = value_from_expr(cx, key.as_expr())?;
        let second_key = lua_integer_value(cx, 2)?;
        rows.push(lua_table_from_values(
            cx,
            vec![(first_key, row_key), (second_key, value)],
        )?);
    }
    cx.factory().list(rows)
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

fn first_arg(args: Vec<Value>, context: &str) -> Result<Value> {
    args.into_iter()
        .next()
        .ok_or_else(|| Error::Eval(format!("{context} requires a value")))
}

fn required_arg(args: &mut Vec<Value>, context: &str) -> Result<Value> {
    if args.is_empty() {
        return Err(Error::Eval(format!("{context} requires a function")));
    }
    Ok(args.remove(0))
}

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
