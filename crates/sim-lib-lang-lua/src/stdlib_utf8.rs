use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{LuaEvalPolicy, LuaNumber, lua_core_profile, lua_integer_value, lua_number_from_value};

#[derive(Clone, Copy)]
pub(crate) enum LuaUtf8Kind {
    Char,
    Codepoint,
    Len,
    Offset,
}

impl LuaUtf8Kind {
    const ALL: [Self; 4] = [Self::Char, Self::Codepoint, Self::Len, Self::Offset];

    fn env_name(self) -> &'static str {
        match self {
            Self::Char => "char",
            Self::Codepoint => "codepoint",
            Self::Len => "len",
            Self::Offset => "offset",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/utf8", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaUtf8Function {
    kind: LuaUtf8Kind,
}

impl LuaUtf8Function {
    fn new(kind: LuaUtf8Kind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaUtf8Kind {
        self.kind
    }
}

impl Object for LuaUtf8Function {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-utf8-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaUtf8Function {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaUtf8Function {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_utf8_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_utf8_stdlib(
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
    for kind in LuaUtf8Kind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaUtf8Function::new(kind)))?;
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
            Symbol::new(format!("utf8.{}", kind.env_name())),
            function,
        )?;
    }
    let charpattern = cx
        .factory()
        .string("[\\0-\\127\\194-\\244][\\128-\\191]*".to_owned())?;
    entries.push((
        cx.factory().string("charpattern".to_owned())?,
        charpattern.clone(),
    ));
    define_or_assign(env, Symbol::new("utf8.charpattern"), charpattern)?;
    define_or_assign(
        env,
        Symbol::new("utf8"),
        crate::lua_table_from_values(cx, entries)?,
    )
}

pub(crate) fn run_lua_utf8_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaUtf8Kind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaUtf8Kind::Char => utf8_char(cx, args),
        LuaUtf8Kind::Codepoint => utf8_codepoint(cx, args),
        LuaUtf8Kind::Len => utf8_len(cx, policy, args),
        LuaUtf8Kind::Offset => utf8_offset(cx, policy, args),
    }
}

fn utf8_char(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let mut out = String::new();
    for value in args {
        let code = integer_arg(cx, &value, "utf8.char codepoint")?;
        let ch = char::from_u32(code as u32)
            .ok_or_else(|| Error::Eval("utf8.char codepoint out of range".to_owned()))?;
        out.push(ch);
    }
    cx.factory().string(out).map(|value| vec![value])
}

fn utf8_codepoint(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "utf8.codepoint")?;
    let len = subject.len();
    let first = args
        .get(1)
        .map(|value| integer_arg(cx, value, "utf8.codepoint first"))
        .transpose()?
        .unwrap_or(1);
    let last = args
        .get(2)
        .map(|value| integer_arg(cx, value, "utf8.codepoint last"))
        .transpose()?
        .unwrap_or(first);
    let start = normalize_byte_index(len, first).clamp(1, len as i64 + 1);
    let end = normalize_byte_index(len, last).clamp(0, len as i64);
    if end < start {
        return Ok(Vec::new());
    }
    let slice = subject
        .get((start - 1) as usize..end as usize)
        .ok_or_else(|| Error::Eval("utf8.codepoint range is not on char boundaries".to_owned()))?;
    slice
        .chars()
        .map(|ch| lua_integer_value(cx, ch as u32 as i64))
        .collect()
}

fn utf8_len(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "utf8.len")?;
    let len = subject.len();
    let first = args
        .get(1)
        .map(|value| integer_arg(cx, value, "utf8.len first"))
        .transpose()?
        .unwrap_or(1);
    let last = args
        .get(2)
        .map(|value| integer_arg(cx, value, "utf8.len last"))
        .transpose()?
        .unwrap_or(-1);
    let start = normalize_byte_index(len, first).clamp(1, len as i64 + 1);
    let end = normalize_byte_index(len, last).clamp(0, len as i64);
    if end < start {
        return Ok(vec![lua_integer_value(cx, 0)?]);
    }
    match subject.get((start - 1) as usize..end as usize) {
        Some(slice) => lua_integer_value(cx, slice.chars().count() as i64).map(|value| vec![value]),
        None => Ok(vec![
            policy.kit().nil.clone(),
            lua_integer_value(cx, start)?,
        ]),
    }
}

fn utf8_offset(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "utf8.offset")?;
    let n = args
        .get(1)
        .map(|value| integer_arg(cx, value, "utf8.offset n"))
        .transpose()?
        .unwrap_or(1);
    let i = args
        .get(2)
        .map(|value| integer_arg(cx, value, "utf8.offset i"))
        .transpose()?
        .unwrap_or(if n >= 0 { 1 } else { subject.len() as i64 + 1 });
    let starts = char_starts(&subject);
    if n == 0 {
        let index = containing_char_start(
            &starts,
            subject.len(),
            normalize_byte_index(subject.len(), i),
        );
        return index
            .map(|offset| lua_integer_value(cx, offset as i64 + 1).map(|value| vec![value]))
            .unwrap_or_else(|| Ok(vec![policy.kit().nil.clone()]));
    }
    let base = normalize_byte_index(subject.len(), i);
    let Some(char_index) = starts.iter().position(|offset| *offset as i64 + 1 >= base) else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    let target = char_index as i64 + if n > 0 { n - 1 } else { n };
    if target < 0 || target as usize >= starts.len() {
        return Ok(vec![policy.kit().nil.clone()]);
    }
    lua_integer_value(cx, starts[target as usize] as i64 + 1).map(|value| vec![value])
}

fn string_arg(cx: &mut Cx, args: &[Value], index: usize, context: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires argument {}", index + 1)))?;
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        _ => Err(Error::Eval(format!("{context} requires a string"))),
    }
}

fn integer_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<i64> {
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => Ok(value),
        Some(LuaNumber::Float(value)) if value.fract() == 0.0 => Ok(value as i64),
        _ => Err(Error::Eval(format!("{context} must be an integer"))),
    }
}

fn normalize_byte_index(len: usize, index: i64) -> i64 {
    if index >= 0 {
        index
    } else {
        len as i64 + index + 1
    }
}

fn char_starts(subject: &str) -> Vec<usize> {
    subject.char_indices().map(|(offset, _)| offset).collect()
}

fn containing_char_start(starts: &[usize], len: usize, index: i64) -> Option<usize> {
    let offset = normalize_byte_index(len, index).max(1) as usize - 1;
    starts
        .iter()
        .copied()
        .take_while(|start| *start <= offset)
        .last()
}

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
