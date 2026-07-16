use std::sync::Arc;

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, DefaultFactory, Demand, EagerPolicy, Error, EvalPolicy,
    EventKind, Expr, PreparedArgs, RawArgs, Result, TrustLevel, Value, read_construct_capability,
    read_eval_capability,
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
    let events = broker.events_for_run(&read_eval_decision_run()).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, EventKind::Trace(_)));
    let decisions = broker.decisions(&cx).unwrap();
    assert_eq!(decisions.len(), 1);
    let decision = &decisions[0];
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
    let events = broker.events_for_run(&read_eval_decision_run()).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, EventKind::Trace(_)));
    let decisions = broker.decisions(&cx).unwrap();
    assert_eq!(decisions.len(), 1);
    let decision = &decisions[0];
    assert_eq!(decision.outcome, ReadEvalOutcome::ShapeDenied);
    assert_cap(&decision.requested, &probe);
    assert_cap(&decision.active, &probe);
    assert_no_cap(&decision.active, &read_eval_capability());
    assert_no_cap(&decision.active, &read_construct_capability());
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
