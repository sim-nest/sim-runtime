use std::sync::Arc;

use sim_kernel::{Args, Callable, ClassRef, Cx, Expr, Object, ObjectCompat, Result, Symbol, Value};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{LuaEvalPolicy, lua_core_profile, lua_table_from_values};

#[derive(Clone, Copy)]
pub(crate) enum LuaDebugKind {
    Traceback,
    SetHook,
    GetLocal,
}

impl LuaDebugKind {
    const ALL: [Self; 3] = [Self::Traceback, Self::SetHook, Self::GetLocal];

    fn env_name(self) -> &'static str {
        match self {
            Self::Traceback => "traceback",
            Self::SetHook => "sethook",
            Self::GetLocal => "getlocal",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/debug", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaDebugFunction {
    kind: LuaDebugKind,
}

impl LuaDebugFunction {
    fn new(kind: LuaDebugKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaDebugKind {
        self.kind
    }
}

impl Object for LuaDebugFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-debug-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaDebugFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaDebugFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_debug_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_debug_stdlib(
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
    for kind in LuaDebugKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaDebugFunction::new(kind)))?;
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
            Symbol::new(format!("debug.{}", kind.env_name())),
            function,
        )?;
    }
    let table = crate::lua_table_from_values(cx, entries)?;
    define_or_assign(env, Symbol::new("debug"), table)
}

pub(crate) fn run_lua_debug_function(
    cx: &mut Cx,
    _policy: &LuaEvalPolicy,
    kind: LuaDebugKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaDebugKind::Traceback => lua_traceback(cx, args),
        LuaDebugKind::SetHook => lua_expected_gap_table(
            cx,
            "lua.debug.sethook",
            "Lua debug hooks are outside the safe debug subset",
        )
        .map(|value| vec![value]),
        LuaDebugKind::GetLocal => lua_expected_gap_table(
            cx,
            "lua.debug.getlocal.foreign-frame",
            "Lua foreign-frame local inspection is outside the safe debug subset",
        )
        .map(|value| vec![value]),
    }
}

pub(crate) fn lua_expected_gap_table(cx: &mut Cx, code: &str, reason: &str) -> Result<Value> {
    lua_table_from_values(
        cx,
        vec![
            (
                cx.factory().string("kind".to_owned())?,
                cx.factory().string("ExpectedGap".to_owned())?,
            ),
            (
                cx.factory().string("code".to_owned())?,
                cx.factory().string(code.to_owned())?,
            ),
            (
                cx.factory().string("reason".to_owned())?,
                cx.factory().string(reason.to_owned())?,
            ),
        ],
    )
}

fn lua_traceback(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let message = args
        .first()
        .map(|value| match value.object().as_expr(cx)? {
            Expr::String(text) => Ok(text),
            Expr::Nil => Ok(String::new()),
            _ => value.object().display(cx),
        })
        .transpose()?
        .unwrap_or_default();
    let text = if message.is_empty() {
        "SIM Lua frame: current chunk".to_owned()
    } else {
        format!("{message}\nSIM Lua frame: current chunk")
    };
    cx.factory().string(text).map(|value| vec![value])
}

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
