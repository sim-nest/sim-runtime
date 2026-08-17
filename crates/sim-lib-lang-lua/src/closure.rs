use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value};
use sim_lib_function::{
    BoundCall, CallMode, CaptureDescriptor, CapturedBinding, FunctionBodyPolicy, FunctionInstance,
    FunctionPlan, ParameterDescriptor, ParameterKind,
};
use sim_lib_standard_core::Arity;

use crate::{LuaEnv, LuaEvalPolicy, LuaResult};

/// Lua-owned body policy layered over the shared function plan and capture graph.
#[derive(Clone)]
pub struct LuaBodyPolicy {
    env: LuaEnv,
    vararg: bool,
    body: Expr,
}

impl FunctionBodyPolicy for LuaBodyPolicy {
    fn invoke(
        &self,
        cx: &mut Cx,
        plan: &FunctionPlan,
        captures: &[CapturedBinding],
        call: BoundCall,
    ) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let args = call
            .arguments()
            .iter()
            .filter_map(|argument| match argument.input() {
                sim_lib_function::ArgumentInput::Positional(value) => Some(value.clone()),
                _ => None,
            })
            .collect();
        let values = invoke_lua_body(cx, &policy, plan, captures, self, args)?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

/// Lua closure using shared declaration, capture, callable, and reachability mechanics.
pub type LuaClosure = FunctionInstance<LuaBodyPolicy>;

/// Lua vararg bundle stored in the special `...` local.
#[derive(Clone)]
pub struct LuaVarargs {
    values: Vec<Value>,
}

impl LuaVarargs {
    fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Return the carried vararg values.
    pub fn values(&self) -> &[Value] {
        &self.values
    }
}

impl Object for LuaVarargs {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-varargs {}>", self.values.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaVarargs {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.values
            .iter()
            .map(|value| value.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()
            .map(Expr::List)
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }
}

pub(crate) fn lua_closure_value(
    cx: &mut Cx,
    env: &LuaEnv,
    name: Symbol,
    params: Vec<Symbol>,
    vararg: bool,
    body: Expr,
    captures: Vec<Symbol>,
) -> Result<Value> {
    let mut parameters = params
        .into_iter()
        .map(|name| {
            ParameterDescriptor::new(name, ParameterKind::Required, CallMode::POSITIONAL, None)
        })
        .collect::<Vec<_>>();
    if vararg {
        parameters.push(ParameterDescriptor::new(
            Symbol::new("..."),
            ParameterKind::Remainder,
            CallMode::POSITIONAL,
            None,
        ));
    }
    let descriptors = captures
        .iter()
        .cloned()
        .map(|name| CaptureDescriptor::new(name, None))
        .collect();
    let captured = captures
        .iter()
        .map(|name| env.capture_managed(name))
        .collect::<Result<Vec<_>>>()?;
    let plan = FunctionPlan::new(name, parameters, descriptors, None)
        .map_err(|error| Error::Eval(error.to_string()))?;
    let class = cx.factory().symbol(Symbol::qualified("core", "Function"))?;
    let closure = LuaClosure::new(
        plan,
        LuaBodyPolicy {
            env: env.clone(),
            vararg,
            body,
        },
        captured,
        class,
        None,
        None,
    )
    .map_err(|error| Error::Eval(error.to_string()))?;
    cx.factory().opaque(Arc::new(closure))
}

pub(crate) fn lua_varargs_value(cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
    cx.factory().opaque(Arc::new(LuaVarargs::new(values)))
}

pub(crate) fn lua_varargs_values(value: &Value) -> Option<Vec<Value>> {
    value
        .object()
        .downcast_ref::<LuaVarargs>()
        .map(|varargs| varargs.values().to_vec())
}

pub(crate) fn call_lua_closure(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    closure: &LuaClosure,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    invoke_lua_body(
        cx,
        policy,
        closure.plan(),
        closure.captures(),
        closure.body(),
        args,
    )
}

fn invoke_lua_body(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    plan: &FunctionPlan,
    captures: &[CapturedBinding],
    body: &LuaBodyPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    debug_assert_eq!(plan.captures().len(), captures.len());
    let mut env = body.env.child();
    let fixed_count = plan
        .parameters()
        .iter()
        .filter(|parameter| parameter.kind() != ParameterKind::Remainder)
        .count();
    let fixed = policy
        .kit()
        .adjust_values(args.clone(), Arity::Exact(fixed_count));
    for (param, value) in plan.parameters().iter().take(fixed_count).zip(fixed) {
        let param = param.name().clone();
        env.define(param, value)?;
    }
    if body.vararg {
        let extras = args.into_iter().skip(fixed_count).collect::<Vec<_>>();
        env.define(Symbol::new("..."), lua_varargs_value(cx, extras)?)?;
    }

    match policy.eval(cx, &mut env, &body.body)? {
        LuaResult::Values(values) | LuaResult::Return(values) => Ok(values),
        LuaResult::Break => Err(Error::Eval(
            "lua break cannot leave a function body".to_owned(),
        )),
    }
}
