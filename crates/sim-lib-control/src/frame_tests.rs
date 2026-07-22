use std::sync::{Arc, Mutex};

use sim_kernel::{
    Args, CORE_FUNCTION_CLASS_ID, Callable, ClassRef, Cx, DefaultFactory, Error, Expr,
    NoopEvalPolicy, Object, ObjectCompat, Ref, Result, RuntimeObject, Symbol, Value,
};

use super::{
    CloseGuard, CoroutineFrame, CoroutineFrameStep, ProtectedOutcome, install_control_policy,
    protected_call, run_with_close_guards,
};

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    install_control_policy(&mut cx);
    cx
}

fn symbol(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn symbol_ref(namespace: &str, name: &str) -> Ref {
    Ref::Symbol(symbol(namespace, name))
}

fn symbol_value(cx: &mut Cx, namespace: &str, name: &str) -> Value {
    cx.factory().symbol(symbol(namespace, name)).unwrap()
}

fn callable_value(cx: &mut Cx, callable: TestCallable) -> Value {
    let object: Arc<dyn RuntimeObject> = Arc::new(callable);
    cx.factory().opaque(object).unwrap()
}

struct TestCallable {
    display: &'static str,
    kind: TestCallableKind,
}

enum TestCallableKind {
    Return(Value),
    Fail(&'static str),
    RecordClose {
        marker: Symbol,
        calls: Arc<Mutex<Vec<CloseCall>>>,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct CloseCall {
    marker: Symbol,
    value: Expr,
    pending_error: Expr,
}

impl Object for TestCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(self.display.to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TestCallable {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(CORE_FUNCTION_CLASS_ID, symbol("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for TestCallable {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        match &self.kind {
            TestCallableKind::Return(value) => Ok(value.clone()),
            TestCallableKind::Fail(message) => Err(Error::Eval((*message).to_owned())),
            TestCallableKind::RecordClose { marker, calls } => {
                let values = args.into_vec();
                let value = values.first().ok_or_else(|| {
                    Error::Eval("close callable expects guarded value".to_owned())
                })?;
                let pending_error = values.get(1).ok_or_else(|| {
                    Error::Eval("close callable expects pending error".to_owned())
                })?;
                calls
                    .lock()
                    .map_err(|_| Error::PoisonedLock("close call records"))?
                    .push(CloseCall {
                        marker: marker.clone(),
                        value: value.object().as_expr(cx)?,
                        pending_error: pending_error.object().as_expr(cx)?,
                    });
                cx.factory().nil()
            }
        }
    }
}

#[test]
fn protected_call_returns_values_and_maps_errors() {
    let mut cx = cx();
    let returned = symbol_value(&mut cx, "test", "returned");
    let function = callable_value(
        &mut cx,
        TestCallable {
            display: "#<return>",
            kind: TestCallableKind::Return(returned.clone()),
        },
    );

    let outcome = protected_call(&mut cx, function, Args::new(Vec::new()), |_cx, _error| {
        unreachable!("returning callable must not map an error")
    })
    .unwrap();

    match outcome {
        ProtectedOutcome::Returned(values) => assert_eq!(values, vec![returned]),
        ProtectedOutcome::Raised(value) => panic!("expected return, got {value:?}"),
    }

    let failure = callable_value(
        &mut cx,
        TestCallable {
            display: "#<fail>",
            kind: TestCallableKind::Fail("protected boundary"),
        },
    );
    let mapped = protected_call(&mut cx, failure, Args::new(Vec::new()), |cx, error| {
        assert!(error.to_string().contains("protected boundary"));
        cx.factory().symbol(symbol("test", "mapped-error"))
    })
    .unwrap();

    match mapped {
        ProtectedOutcome::Returned(values) => panic!("expected raise, got {values:?}"),
        ProtectedOutcome::Raised(value) => {
            assert_eq!(
                value.object().as_expr(&mut cx).unwrap(),
                Expr::Symbol(symbol("test", "mapped-error"))
            );
        }
    }
}

#[test]
fn coroutine_frame_produces_and_consumes_without_surface_names() {
    let mut frame = CoroutineFrame::new(
        vec![
            symbol_ref("producer", "first"),
            symbol_ref("producer", "second"),
        ],
        vec![symbol_ref("consumer", "first")],
    );

    assert_eq!(
        frame.resume(),
        CoroutineFrameStep::Produced(symbol_ref("producer", "first"))
    );
    assert_eq!(
        frame.resume(),
        CoroutineFrameStep::Consumed(symbol_ref("consumer", "first"))
    );
    assert_eq!(
        frame.resume(),
        CoroutineFrameStep::Produced(symbol_ref("producer", "second"))
    );
    assert_eq!(frame.resume(), CoroutineFrameStep::Complete);
    assert!(frame.is_complete());
}

#[test]
fn close_guards_run_on_return_and_error_paths() {
    let mut cx = cx();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first_close = callable_value(
        &mut cx,
        TestCallable {
            display: "#<close-first>",
            kind: TestCallableKind::RecordClose {
                marker: symbol("guard", "first"),
                calls: calls.clone(),
            },
        },
    );
    let second_close = callable_value(
        &mut cx,
        TestCallable {
            display: "#<close-second>",
            kind: TestCallableKind::RecordClose {
                marker: symbol("guard", "second"),
                calls: calls.clone(),
            },
        },
    );
    let first_value = symbol_value(&mut cx, "resource", "first");
    let second_value = symbol_value(&mut cx, "resource", "second");
    let guards = vec![
        CloseGuard::new(first_value, first_close),
        CloseGuard::new(second_value, second_close),
    ];

    let returned = run_with_close_guards(&mut cx, guards.clone(), |cx| {
        Ok(symbol_value(cx, "body", "ok"))
    })
    .unwrap();

    assert_eq!(
        returned.object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(symbol("body", "ok"))
    );
    let normal_calls = calls.lock().unwrap();
    assert_eq!(normal_calls.len(), 2);
    assert_eq!(normal_calls[0].marker, symbol("guard", "second"));
    assert_eq!(normal_calls[0].pending_error, Expr::Nil);
    assert_eq!(normal_calls[1].marker, symbol("guard", "first"));
    assert_eq!(normal_calls[1].pending_error, Expr::Nil);
    drop(normal_calls);

    calls.lock().unwrap().clear();
    let error = run_with_close_guards(&mut cx, guards, |_cx| {
        Err(Error::Eval("body failed".to_owned()))
    })
    .unwrap_err();

    assert!(error.to_string().contains("body failed"));
    let error_calls = calls.lock().unwrap();
    assert_eq!(error_calls.len(), 2);
    assert_eq!(error_calls[0].marker, symbol("guard", "second"));
    assert!(matches!(
        &error_calls[0].pending_error,
        Expr::String(message) if message.contains("body failed")
    ));
    assert_eq!(error_calls[1].marker, symbol("guard", "first"));
    assert!(matches!(
        &error_calls[1].pending_error,
        Expr::String(message) if message.contains("body failed")
    ));
}
