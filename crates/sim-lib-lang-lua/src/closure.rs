use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_binding::BindingCell;
use sim_lib_standard_core::Arity;

use crate::{LuaEnv, LuaEvalPolicy, LuaResult};

/// Lua closure value backed by the shared binding organ.
#[derive(Clone)]
pub struct LuaClosure {
    name: Symbol,
    env: LuaEnv,
    params: Vec<Symbol>,
    vararg: bool,
    body: Expr,
    upvalues: Vec<BindingCell>,
}

impl LuaClosure {
    /// Build a Lua closure over an existing lexical environment.
    pub fn new(
        name: Symbol,
        env: LuaEnv,
        params: Vec<Symbol>,
        vararg: bool,
        body: Expr,
        upvalues: Vec<BindingCell>,
    ) -> Self {
        Self {
            name,
            env,
            params,
            vararg,
            body,
            upvalues,
        }
    }
}

impl Object for LuaClosure {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<lua-closure {} upvalues={}>",
            self.name,
            self.upvalues.len()
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaClosure {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaClosure {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = call_lua_closure(cx, &policy, self, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

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
    let mut upvalues = Vec::with_capacity(captures.len());
    for capture in captures {
        upvalues.push(env.capture(&capture)?);
    }
    cx.factory().opaque(Arc::new(LuaClosure::new(
        name,
        env.clone(),
        params,
        vararg,
        body,
        upvalues,
    )))
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
    let mut env = closure.env.child();
    let fixed = policy
        .kit()
        .adjust_values(args.clone(), Arity::Exact(closure.params.len()));
    for (param, value) in closure.params.iter().cloned().zip(fixed) {
        env.define(param, value)?;
    }
    if closure.vararg {
        let extras = args
            .into_iter()
            .skip(closure.params.len())
            .collect::<Vec<_>>();
        env.define(Symbol::new("..."), lua_varargs_value(cx, extras)?)?;
    }

    match policy.eval(cx, &mut env, &closure.body)? {
        LuaResult::Values(values) | LuaResult::Return(values) => Ok(values),
        LuaResult::Break => Err(Error::Eval(
            "lua break cannot leave a function body".to_owned(),
        )),
    }
}
