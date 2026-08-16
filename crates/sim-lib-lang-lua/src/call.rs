use sim_kernel::{
    Args, ClassId, ClassRef, CodecId, Cx, Origin, Result, SourceId, Span, Symbol, Value,
};
use sim_lib_control::{
    BoundedSubclassOutcome, ClassMatchBudget, ClassMatchEvidence, ClassMatchOutcome,
    ProtectedOutcome, Raised, match_raised_class, protected_call_with,
};

use crate::{
    LuaEvalPolicy,
    closure::{LuaClosure, call_lua_closure},
    load::{LuaLoadFunction, LuaLoadedChunk, call_lua_loaded_chunk, run_lua_load_function},
    stdlib_base::{LuaBaseFunction, run_lua_base_function},
    stdlib_coroutine::{
        LuaCoroutineFunction, LuaCoroutineWrapper, call_lua_coroutine_wrapper,
        run_lua_coroutine_function,
    },
    stdlib_debug::{LuaDebugFunction, run_lua_debug_function},
    stdlib_io::{LuaIoFunction, run_lua_io_function},
    stdlib_math::{LuaMathFunction, run_lua_math_function},
    stdlib_os::{LuaOsFunction, run_lua_os_function},
    stdlib_package::{LuaPackageFunction, run_lua_package_function},
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
    if let Some(chunk) = callee.object().downcast_ref::<LuaLoadedChunk>() {
        return call_lua_loaded_chunk(cx, chunk, args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaBaseFunction>() {
        return run_lua_base_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaLoadFunction>() {
        return run_lua_load_function(cx, function.kind(), args);
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
    if let Some(function) = callee.object().downcast_ref::<LuaMathFunction>() {
        return run_lua_math_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaPackageFunction>() {
        return run_lua_package_function(cx, policy, function, args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaIoFunction>() {
        return run_lua_io_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaOsFunction>() {
        return run_lua_os_function(cx, policy, function.kind(), args);
    }
    if let Some(function) = callee.object().downcast_ref::<LuaDebugFunction>() {
        return run_lua_debug_function(cx, policy, function.kind(), args);
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
) -> Result<ProtectedOutcome<Raised>> {
    let exceptions = LuaExceptionProfile::new(cx)?;
    if function.object().downcast_ref::<LuaClosure>().is_some()
        || function.object().downcast_ref::<LuaLoadedChunk>().is_some()
        || function
            .object()
            .downcast_ref::<LuaBaseFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaLoadFunction>()
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
            .downcast_ref::<LuaMathFunction>()
            .is_some()
        || function
            .object()
            .downcast_ref::<LuaPackageFunction>()
            .is_some()
        || function.object().downcast_ref::<LuaIoFunction>().is_some()
        || function.object().downcast_ref::<LuaOsFunction>().is_some()
        || function
            .object()
            .downcast_ref::<LuaDebugFunction>()
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
            Err(error) => Ok(ProtectedOutcome::Raised(
                exceptions.raise(error_value(cx, error)?, lua_error_origin())?,
            )),
        };
    }

    protected_call_with(cx, function, Args::new(args), |cx, error| {
        exceptions.raise(error_value(cx, error)?, lua_error_origin())
    })
}

/// Lua's one adapter onto the shared exceptional-completion envelope.
pub struct LuaExceptionProfile {
    raised_value_class: ClassRef,
}

impl LuaExceptionProfile {
    /// Builds the profile with Lua's canonical class for arbitrary raised values.
    pub fn new(cx: &Cx) -> Result<Self> {
        Ok(Self {
            raised_value_class: cx.factory().class_stub(
                ClassId(0x4c55_4101),
                Symbol::qualified("lua", "RaisedValue"),
            )?,
        })
    }

    /// Wraps a Lua value without copying or replacing its managed identity.
    pub fn raise(&self, value: Value, origin: Origin) -> Result<Raised> {
        Raised::new(
            self.raised_value_class.clone(),
            value,
            origin,
            Symbol::qualified("lua", "raised-value"),
        )
    }

    /// Lua matches only the canonical raised-value class; it adds no widening predicate.
    pub fn matches(
        &self,
        cx: &mut Cx,
        raised: &Raised,
        candidate: ClassRef,
        budget: ClassMatchBudget,
    ) -> ClassMatchOutcome {
        match_raised_class(
            cx,
            raised,
            candidate,
            budget,
            |_, actual, expected, _| {
                let raised = actual
                    .object()
                    .as_class()
                    .expect("matcher validated class")
                    .id();
                let candidate = expected
                    .object()
                    .as_class()
                    .expect("matcher validated class")
                    .id();
                let evidence = ClassMatchEvidence {
                    raised,
                    candidate,
                    performed_work: 1,
                };
                if raised == candidate {
                    BoundedSubclassOutcome::Subclass(evidence)
                } else {
                    BoundedSubclassOutcome::NotSubclass(evidence)
                }
            },
            |_, _, _| Ok(true),
        )
    }
}

fn lua_error_origin() -> Origin {
    Origin {
        codec: CodecId(0),
        source: SourceId("lua-protected-call".into()),
        span: Span { start: 0, end: 0 },
        trivia: Vec::new(),
    }
}

pub(crate) fn error_value(cx: &mut Cx, error: sim_kernel::Error) -> Result<Value> {
    cx.factory().string(error.to_string())
}

#[cfg(test)]
mod tests {
    use sim_kernel::{CodecId, Origin, SourceId, Span, testing::bare_cx};
    use sim_lib_control::{ClassMatchBudget, ClassMatchOutcome};

    use super::LuaExceptionProfile;
    use crate::lua_table_from_values;

    #[test]
    fn raised_table_keeps_identity_and_uses_explicit_lua_match_policy() {
        let mut cx = bare_cx();
        let profile = LuaExceptionProfile::new(&cx).unwrap();
        let table = lua_table_from_values(&mut cx, Vec::new()).unwrap();
        let origin = Origin {
            codec: CodecId(7),
            source: SourceId("frozen-lua-capture".into()),
            span: Span { start: 4, end: 9 },
            trivia: Vec::new(),
        };
        let raised = profile.raise(table.clone(), origin).unwrap();

        assert_eq!(raised.payload(), &table);
        assert!(matches!(
            profile.matches(
                &mut cx,
                &raised,
                raised.class_ref().clone(),
                ClassMatchBudget { work: 1 },
            ),
            ClassMatchOutcome::Matched(_)
        ));
    }
}
