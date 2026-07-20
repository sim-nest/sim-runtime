use sim_kernel::{Args, Cx, Result, Value};
use sim_lib_control::{ProtectedOutcome, protected_call};

use crate::{
    LuaEvalPolicy,
    closure::{LuaClosure, call_lua_closure},
    stdlib_base::{LuaBaseFunction, run_lua_base_function},
    stdlib_coroutine::{
        LuaCoroutineFunction, LuaCoroutineWrapper, call_lua_coroutine_wrapper,
        run_lua_coroutine_function,
    },
    stdlib_string::{LuaStringFunction, run_lua_string_function},
    stdlib_string_pattern::{LuaGMatchIterator, call_lua_gmatch_iterator},
    stdlib_table::{LuaTableFunction, run_lua_table_function},
    stdlib_utf8::{LuaUtf8Function, run_lua_utf8_function},
};

pub(crate) fn call_lua_value(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    callee: Value,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    if let Some(closure) = callee.object().downcast_ref::<LuaClosure>() {
        return call_lua_closure(cx, policy, closure, args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaBaseFunction>() {
        return run_lua_base_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaCoroutineFunction>() {
        return run_lua_coroutine_function(cx, policy, function.kind(), args);
    }
    if let Some(wrapper) = callee.object().downcast_ref::<LuaCoroutineWrapper>() {
        return call_lua_coroutine_wrapper(cx, policy, wrapper, args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaTableFunction>() {
        return run_lua_table_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaStringFunction>() {
        return run_lua_string_function(cx, policy, function.kind(), args);
    }
    if let Some(iterator) = callee.object().downcast_ref::<LuaGMatchIterator>() {
        return call_lua_gmatch_iterator(cx, policy, iterator);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaUtf8Function>() {
        return run_lua_utf8_function(cx, policy, function.kind(), args);
    }
    cx.call_value(callee, Args::new(args))
        .map(|value| vec![value])
}

pub(crate) fn protected_lua_call(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    function: Value,
    args: Vec<Value>,
) -> Result<ProtectedOutcome> {
    if function.object().downcast_ref::<LuaClosure>().is_some()
        || function
            .object()
            .downcast_ref::<LuaBaseFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaCoroutineFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaCoroutineWrapper>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaTableFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaStringFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaGMatchIterator>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaUtf8Function>()
            .is_some()
    {
        return match call_lua_value(cx, policy, function, args) {
            Ok(values) => Ok(ProtectedOutcome::Returned(values)),
            Err(error) => Ok(ProtectedOutcome::Raised(error_value(cx, error)?)),
        };
    }

    protected_call(cx, function, Args::new(args), error_value)
}

pub(crate) fn error_value(cx: &mut Cx, error: sim_kernel::Error) -> Result<Value> {
    cx.factory().string(error.to_string())
}
