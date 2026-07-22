use std::sync::{Arc, Mutex};

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_control::{CoroutineFrame, CoroutineFrameStep};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{LuaEvalPolicy, call::call_lua_value, lua_core_profile, lua_table_from_values};

#[derive(Clone, Copy)]
pub(crate) enum LuaCoroutineKind {
    Create,
    Resume,
    Yield,
    Status,
    Wrap,
    IsYieldable,
    Running,
}

impl LuaCoroutineKind {
    const ALL: [Self; 7] = [
        Self::Create,
        Self::Resume,
        Self::Yield,
        Self::Status,
        Self::Wrap,
        Self::IsYieldable,
        Self::Running,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Resume => "resume",
            Self::Yield => "yield",
            Self::Status => "status",
            Self::Wrap => "wrap",
            Self::IsYieldable => "isyieldable",
            Self::Running => "running",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/coroutine", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaCoroutineFunction {
    kind: LuaCoroutineKind,
}

impl LuaCoroutineFunction {
    fn new(kind: LuaCoroutineKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaCoroutineKind {
        self.kind
    }
}

impl Object for LuaCoroutineFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<lua-coroutine-function {}>",
            self.kind.env_name()
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaCoroutineFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaCoroutineFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_coroutine_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, sim_lib_standard_core::Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

/// Lua coroutine handle.
pub struct LuaThread {
    state: Mutex<LuaThreadState>,
}

#[derive(Clone)]
enum LuaThreadState {
    New(Value),
    Frame(CoroutineFrame<Value>),
    Dead,
}

impl LuaThread {
    fn new(function: Value) -> Self {
        Self {
            state: Mutex::new(LuaThreadState::New(function)),
        }
    }

    fn frame(produced: Vec<Value>, consumed: Vec<Value>) -> Self {
        Self {
            state: Mutex::new(LuaThreadState::Frame(CoroutineFrame::new(
                produced, consumed,
            ))),
        }
    }

    fn status(&self) -> Result<&'static str> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lua coroutine"))?;
        Ok(match &*state {
            LuaThreadState::New(_) | LuaThreadState::Frame(_) => "suspended",
            LuaThreadState::Dead => "dead",
        })
    }

    fn resume(&self, cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
        let state = {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lua coroutine"))?;
            std::mem::replace(&mut *guard, LuaThreadState::Dead)
        };
        match state {
            LuaThreadState::New(function) => match call_lua_value(cx, policy, function, args) {
                Ok(values) => {
                    let mut out = vec![cx.factory().bool(true)?];
                    out.extend(values);
                    Ok(out)
                }
                Err(error) => Ok(vec![
                    cx.factory().bool(false)?,
                    cx.factory().string(error.to_string())?,
                ]),
            },
            LuaThreadState::Frame(mut frame) => match frame.resume() {
                CoroutineFrameStep::Produced(value) | CoroutineFrameStep::Consumed(value) => {
                    let done = frame.is_complete();
                    *self
                        .state
                        .lock()
                        .map_err(|_| Error::PoisonedLock("lua coroutine"))? = if done {
                        LuaThreadState::Dead
                    } else {
                        LuaThreadState::Frame(frame)
                    };
                    Ok(vec![cx.factory().bool(true)?, value])
                }
                CoroutineFrameStep::Complete => Ok(vec![cx.factory().bool(true)?]),
            },
            LuaThreadState::Dead => Ok(vec![
                cx.factory().bool(false)?,
                cx.factory()
                    .string("cannot resume dead coroutine".to_owned())?,
            ]),
        }
    }
}

impl Object for LuaThread {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-thread {}>", self.status()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaThread {
    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }
}

#[derive(Clone)]
pub(crate) struct LuaCoroutineWrapper {
    thread: Arc<LuaThread>,
}

impl LuaCoroutineWrapper {
    fn new(function: Value) -> Self {
        Self {
            thread: Arc::new(LuaThread::new(function)),
        }
    }
}

impl Object for LuaCoroutineWrapper {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<lua-coroutine-wrapper {}>",
            self.thread.status()?
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaCoroutineWrapper {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaCoroutineWrapper {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = call_lua_coroutine_wrapper(cx, &policy, self, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

/// Build a Lua coroutine handle over a shared producer/consumer frame.
pub fn lua_coroutine_frame_value(
    cx: &mut Cx,
    produced: Vec<Value>,
    consumed: Vec<Value>,
) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(LuaThread::frame(produced, consumed)))
}

pub(crate) fn install_lua_coroutine_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let mut table_entries = Vec::new();
    for kind in LuaCoroutineKind::ALL {
        let function = cx
            .factory()
            .opaque(Arc::new(LuaCoroutineFunction::new(kind)))?;
        runtime.define_function(
            &profile_symbol,
            sim_lib_control::control_organ_symbol(),
            kind.function_symbol(),
            function.clone(),
        )?;
        table_entries.push((
            cx.factory().string(kind.env_name().to_owned())?,
            function.clone(),
        ));
        define_or_assign(
            env,
            Symbol::new(format!("coroutine.{}", kind.env_name())),
            function,
        )?;
    }
    let table = lua_table_from_values(cx, table_entries)?;
    define_or_assign(env, Symbol::new("coroutine"), table)
}

pub(crate) fn run_lua_coroutine_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaCoroutineKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaCoroutineKind::Create => {
            let function = first_arg(args, "coroutine.create")?;
            cx.factory()
                .opaque(Arc::new(LuaThread::new(function)))
                .map(|value| vec![value])
        }
        LuaCoroutineKind::Resume => {
            let mut args = args;
            let thread = required_arg(&mut args, "coroutine.resume")?;
            let thread = lua_thread_value(&thread)?;
            thread.resume(cx, policy, args)
        }
        LuaCoroutineKind::Yield => Ok(args),
        LuaCoroutineKind::Status => {
            let value = first_arg(args, "coroutine.status")?;
            let thread = lua_thread_value(&value)?;
            cx.factory()
                .string(thread.status()?.to_owned())
                .map(|value| vec![value])
        }
        LuaCoroutineKind::Wrap => {
            let function = first_arg(args, "coroutine.wrap")?;
            cx.factory()
                .opaque(Arc::new(LuaCoroutineWrapper::new(function)))
                .map(|value| vec![value])
        }
        LuaCoroutineKind::IsYieldable => cx.factory().bool(true).map(|value| vec![value]),
        LuaCoroutineKind::Running => Ok(vec![policy.kit().nil.clone(), cx.factory().bool(false)?]),
    }
}

pub(crate) fn call_lua_coroutine_wrapper(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    wrapper: &LuaCoroutineWrapper,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let mut values = wrapper.thread.resume(cx, policy, args)?.into_iter();
    let status = values
        .next()
        .ok_or_else(|| Error::Eval("coroutine wrapper resume returned no status".to_owned()))?;
    match status.object().as_expr(cx)? {
        Expr::Bool(true) => Ok(values.collect()),
        Expr::Bool(false) => {
            let message = values
                .next()
                .map(|value| value.object().display(cx))
                .transpose()?
                .unwrap_or_else(|| "coroutine error".to_owned());
            Err(Error::Eval(message))
        }
        _ => Err(Error::Eval(
            "coroutine wrapper resume returned non-boolean status".to_owned(),
        )),
    }
}

fn lua_thread_value(value: &Value) -> Result<&LuaThread> {
    value
        .object()
        .downcast_ref::<LuaThread>()
        .ok_or(Error::TypeMismatch {
            expected: "lua coroutine thread",
            found: "non-thread",
        })
}

fn first_arg(args: Vec<Value>, context: &str) -> Result<Value> {
    args.into_iter()
        .next()
        .ok_or_else(|| Error::Eval(format!("{context} requires a value")))
}

fn required_arg(args: &mut Vec<Value>, context: &str) -> Result<Value> {
    if args.is_empty() {
        return Err(Error::Eval(format!("{context} requires a value")));
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
