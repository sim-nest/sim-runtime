use std::sync::Arc;

use sim_kernel::{
    Args, Callable, CapabilityName, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result,
    Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{
    LuaEvalPolicy, lua_core_profile, lua_table_from_values, stdlib_debug::lua_expected_gap_table,
};

#[derive(Clone, Copy)]
pub(crate) enum LuaIoKind {
    Open,
    Input,
    Output,
    Read,
    Write,
    Lines,
    Type,
}

impl LuaIoKind {
    const ALL: [Self; 7] = [
        Self::Open,
        Self::Input,
        Self::Output,
        Self::Read,
        Self::Write,
        Self::Lines,
        Self::Type,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Input => "input",
            Self::Output => "output",
            Self::Read => "read",
            Self::Write => "write",
            Self::Lines => "lines",
            Self::Type => "type",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/io", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaIoFunction {
    kind: LuaIoKind,
}

impl LuaIoFunction {
    fn new(kind: LuaIoKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaIoKind {
        self.kind
    }
}

impl Object for LuaIoFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-io-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaIoFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaIoFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_io_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_io_stdlib(
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
    for kind in LuaIoKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaIoFunction::new(kind)))?;
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
            Symbol::new(format!("io.{}", kind.env_name())),
            function,
        )?;
    }
    let table = lua_table_from_values(cx, entries)?;
    define_or_assign(env, Symbol::new("io"), table)
}

pub(crate) fn run_lua_io_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaIoKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaIoKind::Open => lua_io_open(cx, args),
        LuaIoKind::Input | LuaIoKind::Read | LuaIoKind::Lines => {
            cx.require(&fs_read_capability())?;
            io_gap(cx, "read").map(|value| vec![value])
        }
        LuaIoKind::Output | LuaIoKind::Write => {
            cx.require(&fs_write_capability())?;
            io_gap(cx, "write").map(|value| vec![value])
        }
        LuaIoKind::Type => Ok(vec![policy.kit().nil.clone()]),
    }
}

fn lua_io_open(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let _path = string_arg(cx, &args, 0, "io.open path")?;
    let mode = args
        .get(1)
        .map(|value| string_value(cx, value))
        .transpose()?
        .unwrap_or_else(|| "r".to_owned());
    if mode.contains('w') || mode.contains('a') || mode.contains('+') {
        cx.require(&fs_write_capability())?;
    } else {
        cx.require(&fs_read_capability())?;
    }
    io_gap(cx, "open").map(|value| vec![value])
}

fn io_gap(cx: &mut Cx, operation: &str) -> Result<Value> {
    lua_expected_gap_table(
        cx,
        &format!("lua.io.{operation}.table-dir"),
        "Lua file handles require a Table/Dir-backed file surface",
    )
}

fn fs_read_capability() -> CapabilityName {
    CapabilityName::new("fs/read")
}

fn fs_write_capability() -> CapabilityName {
    CapabilityName::new("fs/write")
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

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
