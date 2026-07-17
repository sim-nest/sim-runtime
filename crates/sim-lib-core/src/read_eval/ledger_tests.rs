use std::sync::Arc;

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, DefaultFactory, Demand, EagerPolicy, Error, EvalPolicy,
    EventKind, Expr, PreparedArgs, RawArgs, Result, Shape, ShapeDoc, ShapeMatch, TrustLevel, Value,
    read_construct_capability, read_eval_capability,
};
use sim_shape::{AnyShape, ExprKind, ExprKindShape};

use super::*;

fn origin() -> RequestOrigin {
    RequestOrigin::with_detail(
        Symbol::qualified("test", "origin"),
        Expr::String("ledger-case".to_owned()),
    )
}

fn policy(trust: TrustLevel, capabilities: CapabilitySet) -> ReadPolicy {
    ReadPolicy {
        trust,
        capabilities,
    }
}

fn trusted_read_eval_policy() -> ReadPolicy {
    policy(
        TrustLevel::TrustedSource,
        CapabilitySet::new().grant(read_eval_capability()),
    )
}

fn request_with(
    source: ReadEvalSource,
    expected_shape: Arc<dyn Shape>,
    allow: CapabilitySet,
) -> ReadEvalRequest {
    ReadEvalRequest {
        origin: origin(),
        codec: Symbol::qualified("codec", "lisp"),
        source,
        read_policy: trusted_read_eval_policy(),
        requires: Vec::new(),
        allow,
        expected_shape,
    }
}

struct ActiveCapabilityPolicy {
    capability: CapabilityName,
}

impl EvalPolicy for ActiveCapabilityPolicy {
    fn name(&self) -> &'static str {
        "read-eval-ledger-active-capability-probe"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        EagerPolicy.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        EagerPolicy.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, _expr: Expr) -> Result<Value> {
        cx.factory()
            .bool(cx.capabilities().contains(&self.capability))
    }
}

fn probe_cx(capability: CapabilityName) -> (Cx, sim_kernel::GrantSeat) {
    Cx::new_seated(
        Arc::new(ActiveCapabilityPolicy { capability }),
        Arc::new(DefaultFactory),
    )
}

struct EvalFailurePolicy {
    error: Error,
}

impl EvalPolicy for EvalFailurePolicy {
    fn name(&self) -> &'static str {
        "read-eval-ledger-failure-probe"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        EagerPolicy.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        EagerPolicy.force(cx, value, demand)
    }

    fn eval_expr(&self, _cx: &mut Cx, _expr: Expr) -> Result<Value> {
        Err(self.error.clone())
    }
}

struct ShapeErrorShape;

impl Shape for ShapeErrorShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Err(Error::Eval("shape check failed".to_owned()))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Err(Error::Eval("shape check failed".to_owned()))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("shape-error"))
    }
}

fn decision_for_run(broker: &ReadEvalBroker, cx: &Cx) -> ReadEvalDecision {
    let events = broker.events_for_run(&read_eval_decision_run()).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, EventKind::Trace(_)));
    let decisions = broker.decisions(cx).unwrap();
    assert_eq!(decisions.len(), 1);
    decisions.into_iter().next().unwrap()
}

#[test]
fn admitted_request_records_trace_decision_with_diminished_caps() {
    let probe = CapabilityName::new("test.probe");
    let (mut cx, seat) = probe_cx(probe.clone());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    expect_granted!(seat.grant(&mut cx, read_construct_capability()));
    expect_granted!(seat.grant(&mut cx, probe.clone()));
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(AnyShape),
        CapabilitySet::new()
            .grant(read_eval_capability())
            .grant(probe.clone()),
    );

    let value = broker.admit(&mut cx, request).unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::Admitted);
    assert_eq!(decision.origin, origin());
    assert_eq!(decision.codec, Symbol::qualified("codec", "lisp"));
    assert_eq!(
        decision.expected_shape,
        Some(Symbol::qualified("core", "Any"))
    );
    assert_cap(&decision.requested, &read_eval_capability());
    assert_cap(&decision.requested, &probe);
    assert_cap(&decision.active, &read_eval_capability());
    assert_cap(&decision.active, &probe);
    assert_no_cap(&decision.active, &read_construct_capability());
}

#[test]
fn denied_request_records_trace_decision_with_diminished_caps() {
    let probe = CapabilityName::new("test.probe");
    let (mut cx, seat) = probe_cx(probe.clone());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    expect_granted!(seat.grant(&mut cx, read_construct_capability()));
    expect_granted!(seat.grant(&mut cx, probe.clone()));
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::String)),
        CapabilitySet::new().grant(probe.clone()),
    );

    let err = broker.admit(&mut cx, request).unwrap_err();

    assert!(matches!(err, Error::WrongShape { .. }));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::ShapeDenied);
    assert_cap(&decision.requested, &probe);
    assert_cap(&decision.active, &probe);
    assert_no_cap(&decision.active, &read_eval_capability());
    assert_no_cap(&decision.active, &read_construct_capability());
}

#[test]
fn malformed_source_records_decode_failure() {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let lib = LispCodecLib::new(sim_kernel::CodecId(1)).unwrap();
    cx.load_lib(&lib).unwrap();
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Text("(unterminated".to_owned()),
        Arc::new(AnyShape),
        CapabilitySet::new().grant(read_eval_capability()),
    );

    let err = broker.admit(&mut cx, request).unwrap_err();

    assert!(matches!(err, Error::CodecError { .. }));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::DecodeFailed);
}

#[test]
fn eval_failure_records_eval_failed_outcome() {
    let (mut cx, seat) = Cx::new_seated(
        Arc::new(EvalFailurePolicy {
            error: Error::Eval("eval failed".to_owned()),
        }),
        Arc::new(DefaultFactory),
    );
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(AnyShape),
        CapabilitySet::new().grant(read_eval_capability()),
    );

    let err = broker.admit(&mut cx, request).unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message == "eval failed"));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::EvalFailed);
}

#[test]
fn eval_time_capability_denial_records_eval_failed_outcome() {
    let denied = CapabilityName::new("test.denied");
    let (mut cx, seat) = Cx::new_seated(
        Arc::new(EvalFailurePolicy {
            error: Error::CapabilityDenied {
                capability: denied.clone(),
            },
        }),
        Arc::new(DefaultFactory),
    );
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    expect_granted!(seat.grant(&mut cx, denied.clone()));
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(AnyShape),
        CapabilitySet::new().grant(read_eval_capability()),
    );

    let err = broker.admit(&mut cx, request).unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == denied
    ));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::EvalFailed);
    assert_no_cap(&decision.active, &denied);
}

#[test]
fn shape_check_error_records_shape_error_outcome() {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let broker = ReadEvalBroker::new();
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ShapeErrorShape),
        CapabilitySet::new().grant(read_eval_capability()),
    );

    let err = broker.admit(&mut cx, request).unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message == "shape check failed"));
    let decision = decision_for_run(&broker, &cx);
    assert_eq!(decision.outcome, ReadEvalOutcome::ShapeError);
}

fn assert_cap(capabilities: &[CapabilityName], expected: &CapabilityName) {
    assert!(
        capabilities.iter().any(|capability| capability == expected),
        "missing capability {expected}; got {capabilities:?}"
    );
}

fn assert_no_cap(capabilities: &[CapabilityName], denied: &CapabilityName) {
    assert!(
        capabilities.iter().all(|capability| capability != denied),
        "unexpected capability {denied}; got {capabilities:?}"
    );
}
