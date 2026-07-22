use std::sync::Arc;

use sim_kernel::{
    Args, Callable, CapabilityName, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result,
    Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{LuaEvalPolicy, lua_core_profile, lua_float_value, lua_table_from_values};

#[derive(Clone, Copy)]
pub(crate) enum LuaOsKind {
    Execute,
    Getenv,
    Clock,
}

impl LuaOsKind {
    const ALL: [Self; 3] = [Self::Execute, Self::Getenv, Self::Clock];

    fn env_name(self) -> &'static str {
        match self {
            Self::Execute => "execute",
            Self::Getenv => "getenv",
            Self::Clock => "clock",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/os", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaOsFunction {
    kind: LuaOsKind,
}

impl LuaOsFunction {
    fn new(kind: LuaOsKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaOsKind {
        self.kind
    }
}

impl Object for LuaOsFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-os-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaOsFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaOsFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_os_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_os_stdlib(
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
    for kind in LuaOsKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaOsFunction::new(kind)))?;
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
            Symbol::new(format!("os.{}", kind.env_name())),
            function,
        )?;
    }
    let table = lua_table_from_values(cx, entries)?;
    define_or_assign(env, Symbol::new("os"), table)
}

pub(crate) fn run_lua_os_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaOsKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaOsKind::Execute => lua_os_execute(cx, args),
        LuaOsKind::Getenv => lua_os_getenv(cx, policy, args),
        LuaOsKind::Clock => lua_float_value(cx, 0.0).map(|value| vec![value]),
    }
}

fn lua_os_execute(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let command = string_arg(cx, &args, 0, "os.execute command")?;
    let argv = vec!["sh".to_owned(), "-c".to_owned(), command];
    let opts = sim_lib_exec::ExecOptions::new(30_000, 64 * 1024);
    let result = sim_lib_exec::exec(cx, &argv, &opts)?;
    cx.factory()
        .bool(result.exit_code == 0)
        .map(|value| vec![value])
}

fn lua_os_getenv(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    cx.require(&env_read_capability())?;
    let name = string_arg(cx, &args, 0, "os.getenv name")?;
    match std::env::var(name) {
        Ok(value) => cx.factory().string(value).map(|value| vec![value]),
        Err(_) => Ok(vec![policy.kit().nil.clone()]),
    }
}

fn env_read_capability() -> CapabilityName {
    CapabilityName::new("env/read")
}

fn string_arg(cx: &mut Cx, args: &[Value], index: usize, context: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires a string")))?;
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
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
