use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{
    LuaEvalPolicy, LuaNumber, lua_core_profile, lua_integer_value, lua_number_from_value,
    stdlib_string_format::lua_string_format,
    stdlib_string_pattern::{
        lua_string_find, lua_string_gmatch, lua_string_gsub, lua_string_match,
    },
};

#[derive(Clone, Copy)]
pub(crate) enum LuaStringKind {
    Byte,
    Char,
    Dump,
    Find,
    Format,
    GMatch,
    GSub,
    Len,
    Lower,
    Match,
    Rep,
    Reverse,
    Sub,
    Upper,
}

impl LuaStringKind {
    const ALL: [Self; 14] = [
        Self::Byte,
        Self::Char,
        Self::Dump,
        Self::Find,
        Self::Format,
        Self::GMatch,
        Self::GSub,
        Self::Len,
        Self::Lower,
        Self::Match,
        Self::Rep,
        Self::Reverse,
        Self::Sub,
        Self::Upper,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::Byte => "byte",
            Self::Char => "char",
            Self::Dump => "dump",
            Self::Find => "find",
            Self::Format => "format",
            Self::GMatch => "gmatch",
            Self::GSub => "gsub",
            Self::Len => "len",
            Self::Lower => "lower",
            Self::Match => "match",
            Self::Rep => "rep",
            Self::Reverse => "reverse",
            Self::Sub => "sub",
            Self::Upper => "upper",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/string", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaStringFunction {
    kind: LuaStringKind,
}

impl LuaStringFunction {
    fn new(kind: LuaStringKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaStringKind {
        self.kind
    }
}

impl Object for LuaStringFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-string-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaStringFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaStringFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_string_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_string_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let mut entries = Vec::new();
    for kind in LuaStringKind::ALL {
        let function = cx
            .factory()
            .opaque(Arc::new(LuaStringFunction::new(kind)))?;
        runtime.define_function(
            &profile_symbol,
            sim_lib_dispatch::dispatch_organ_symbol(),
            kind.function_symbol(),
            function.clone(),
        )?;
        entries.push((
            cx.factory().string(kind.env_name().to_owned())?,
            function.clone(),
        ));
        define_or_assign(
            env,
            Symbol::new(format!("string.{}", kind.env_name())),
            function,
        )?;
    }
    define_or_assign(
        env,
        Symbol::new("string"),
        crate::lua_table_from_values(cx, entries)?,
    )
}

pub(crate) fn run_lua_string_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaStringKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaStringKind::Byte => lua_string_byte(cx, args),
        LuaStringKind::Char => lua_string_char(cx, args),
        LuaStringKind::Dump => lua_string_dump(cx, args),
        LuaStringKind::Find => lua_string_find(cx, policy, args),
        LuaStringKind::Format => lua_string_format(cx, args),
        LuaStringKind::GMatch => lua_string_gmatch(cx, args),
        LuaStringKind::GSub => lua_string_gsub(cx, policy, args),
        LuaStringKind::Len => unary_string(cx, args, "string.len", |text| text.len().to_string()),
        LuaStringKind::Lower => unary_string(cx, args, "string.lower", |text| text.to_lowercase()),
        LuaStringKind::Match => lua_string_match(cx, policy, args),
        LuaStringKind::Rep => lua_string_rep(cx, args),
        LuaStringKind::Reverse => unary_string(cx, args, "string.reverse", |text| {
            text.chars().rev().collect()
        }),
        LuaStringKind::Sub => lua_string_sub(cx, args),
        LuaStringKind::Upper => unary_string(cx, args, "string.upper", |text| text.to_uppercase()),
    }
}

fn lua_string_byte(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.byte")?;
    let bytes = subject.as_bytes();
    let first = args
        .get(1)
        .map(|value| integer_arg(cx, value, "string.byte first"))
        .transpose()?
        .unwrap_or(1);
    let last = args
        .get(2)
        .map(|value| integer_arg(cx, value, "string.byte last"))
        .transpose()?
        .unwrap_or(first);
    let (start, end) = byte_range(bytes.len(), first, last);
    (start..end)
        .map(|index| lua_integer_value(cx, bytes[index] as i64))
        .collect()
}

fn lua_string_char(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let mut out = String::new();
    for value in args {
        let code = integer_arg(cx, &value, "string.char codepoint")?;
        let ch = char::from_u32(code as u32)
            .ok_or_else(|| Error::Eval("string.char codepoint out of range".to_owned()))?;
        out.push(ch);
    }
    cx.factory().string(out).map(|value| vec![value])
}

fn lua_string_dump(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    if args.is_empty() {
        return Err(Error::Eval("string.dump requires a function".to_owned()));
    }
    let entries = vec![
        (
            cx.factory().string("kind".to_owned())?,
            cx.factory().string("ExpectedGap".to_owned())?,
        ),
        (
            cx.factory().string("code".to_owned())?,
            cx.factory().string("lua.bytecode.dump".to_owned())?,
        ),
        (
            cx.factory().string("reason".to_owned())?,
            cx.factory().string(
                "Lua bytecode dumping is not available in this source runtime".to_owned(),
            )?,
        ),
    ];
    crate::lua_table_from_values(cx, entries).map(|value| vec![value])
}

fn lua_string_rep(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.rep")?;
    let count = integer_arg(
        cx,
        args.get(1)
            .ok_or_else(|| Error::Eval("string.rep requires a count".to_owned()))?,
        "string.rep count",
    )?;
    let sep = args
        .get(2)
        .map(|value| lua_to_string(cx, value, "string.rep separator"))
        .transpose()?
        .unwrap_or_default();
    if count <= 0 {
        return cx.factory().string(String::new()).map(|value| vec![value]);
    }
    cx.factory()
        .string(vec![subject; count as usize].join(&sep))
        .map(|value| vec![value])
}

fn lua_string_sub(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.sub")?;
    let first = integer_arg(
        cx,
        args.get(1)
            .ok_or_else(|| Error::Eval("string.sub requires a start".to_owned()))?,
        "string.sub start",
    )?;
    let last = args
        .get(2)
        .map(|value| integer_arg(cx, value, "string.sub end"))
        .transpose()?
        .unwrap_or(-1);
    let (start, end) = byte_range(subject.len(), first, last);
    let text = String::from_utf8_lossy(&subject.as_bytes()[start..end]).into_owned();
    cx.factory().string(text).map(|value| vec![value])
}

fn unary_string(
    cx: &mut Cx,
    args: Vec<Value>,
    context: &str,
    f: impl FnOnce(String) -> String,
) -> Result<Vec<Value>> {
    let text = string_arg(cx, &args, 0, context)?;
    cx.factory().string(f(text)).map(|value| vec![value])
}

pub(crate) fn lua_to_string(cx: &mut Cx, value: &Value, context: &str) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        Expr::Number(number) => Ok(number.canonical),
        Expr::Bool(value) => Ok(if value { "true" } else { "false" }.to_owned()),
        Expr::Nil => Ok("nil".to_owned()),
        _ => Err(Error::Eval(format!("{context} must be string-coercible"))),
    }
}

pub(crate) fn string_arg(
    cx: &mut Cx,
    args: &[Value],
    index: usize,
    context: &str,
) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires argument {}", index + 1)))?;
    lua_to_string(cx, value, context)
}

pub(crate) fn integer_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<i64> {
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => Ok(value),
        Some(LuaNumber::Float(value)) if value.fract() == 0.0 => Ok(value as i64),
        _ => Err(Error::Eval(format!("{context} must be an integer"))),
    }
}

fn byte_range(len: usize, first: i64, last: i64) -> (usize, usize) {
    let start = normalize_index(len, first).clamp(1, len as i64 + 1);
    let end = normalize_index(len, last).clamp(0, len as i64);
    if end < start {
        return (0, 0);
    }
    ((start - 1) as usize, end as usize)
}

fn normalize_index(len: usize, index: i64) -> i64 {
    if index >= 0 {
        index
    } else {
        len as i64 + index + 1
    }
}

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
