use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::codec_normalize::normalize_lua_codec_expr;
use crate::{LuaEnv, LuaEvalPolicy, LuaResult, lua_core_profile};

#[derive(Clone, Copy)]
pub(crate) enum LuaLoadKind {
    Load,
}

impl LuaLoadKind {
    fn env_name(self) -> &'static str {
        match self {
            Self::Load => "load",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/load", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaLoadFunction {
    kind: LuaLoadKind,
}

impl LuaLoadFunction {
    fn new(kind: LuaLoadKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaLoadKind {
        self.kind
    }
}

impl Object for LuaLoadFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-load-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaLoadFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaLoadFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let values = run_lua_load_function(cx, self.kind, args.into_vec())?;
        Ok(values
            .into_iter()
            .next()
            .unwrap_or_else(|| cx.factory().nil().unwrap()))
    }
}

#[derive(Clone)]
pub(crate) struct LuaLoadedChunk {
    name: String,
    body: Expr,
    env_value: Option<Value>,
}

impl LuaLoadedChunk {
    fn new(name: String, body: Expr, env_value: Option<Value>) -> Self {
        Self {
            name,
            body,
            env_value,
        }
    }
}

impl Object for LuaLoadedChunk {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-loaded-chunk {}>", self.name))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaLoadedChunk {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaLoadedChunk {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = call_lua_loaded_chunk(cx, self, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_load_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let kind = LuaLoadKind::Load;
    let function = cx.factory().opaque(Arc::new(LuaLoadFunction::new(kind)))?;
    runtime.define_function(
        &profile_symbol,
        sim_lib_dispatch::dispatch_organ_symbol(),
        kind.function_symbol(),
        function.clone(),
    )?;
    define_or_assign(env, Symbol::new(kind.env_name()), function)
}

pub(crate) fn run_lua_load_function(
    cx: &mut Cx,
    kind: LuaLoadKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaLoadKind::Load => lua_load(cx, args),
    }
}

pub(crate) fn call_lua_loaded_chunk(
    cx: &mut Cx,
    chunk: &LuaLoadedChunk,
    _args: Vec<Value>,
) -> Result<Vec<Value>> {
    let policy = LuaEvalPolicy::new(cx)?;
    let mut env = LuaEnv::new();
    policy.install_stdlib(cx, &mut env)?;
    if let Some(env_value) = &chunk.env_value {
        env.define(Symbol::new("_ENV"), env_value.clone())?;
    }
    match policy.eval(cx, &mut env, &chunk.body)? {
        LuaResult::Values(values) | LuaResult::Return(values) => Ok(values),
        LuaResult::Break => Err(Error::Eval(
            "lua break cannot leave a loaded chunk".to_owned(),
        )),
    }
}

pub(crate) fn eval_lua_source(cx: &mut Cx, source: &str) -> Result<Vec<Value>> {
    let policy = LuaEvalPolicy::new(cx)?;
    let mut env = LuaEnv::new();
    policy.install_stdlib(cx, &mut env)?;
    let expr = decode_lua_source_expr(source)?;
    match policy.eval(cx, &mut env, &expr)? {
        LuaResult::Values(values) | LuaResult::Return(values) => Ok(values),
        LuaResult::Break => Err(Error::Eval("lua break cannot leave source".to_owned())),
    }
}

fn lua_load(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let source = string_arg(cx, &args, 0, "load source")?;
    let chunk_name = args
        .get(1)
        .map(|value| string_value(cx, value))
        .transpose()?
        .unwrap_or_else(|| "=(load)".to_owned());
    let mode = args
        .get(2)
        .map(|value| string_value(cx, value))
        .transpose()?;
    if mode.as_deref() == Some("b") {
        return Err(Error::Eval("Lua bytecode load is not supported".to_owned()));
    }
    let env_value = args.get(3).cloned();
    let body = decode_lua_source_expr(&source)?;
    cx.factory()
        .opaque(Arc::new(LuaLoadedChunk::new(chunk_name, body, env_value)))
        .map(|value| vec![value])
}

fn decode_lua_source_expr(source: &str) -> Result<Expr> {
    let chunk = sim_codec_lua::parse_lua_chunk(source)?;
    Ok(normalize_lua_codec_expr(sim_codec_lua::lower_lua_chunk(
        &chunk,
    )))
}

fn string_arg(cx: &mut Cx, args: &[Value], index: usize, context: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires a string")))?;
    string_value(cx, value)
}

fn string_value(cx: &mut Cx, value: &Value) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
    }
}

fn define_or_assign(env: &mut LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
