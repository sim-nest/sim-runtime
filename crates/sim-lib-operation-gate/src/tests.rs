//! Operation-gate conformance tests.

use super::*;
use sim_kernel::{
    Symbol,
    effect::{effect_abort_op_key, effect_resume_op_key},
    testing::bare_cx,
};
use std::cell::{Cell, RefCell};

struct Verifier(bool);
impl ApprovalVerifier for Verifier {
    fn verify(&self, _: &Approval) -> Result<()> {
        if self.0 {
            Ok(())
        } else {
            Err(Error::Eval("invalid or expired approval".into()))
        }
    }
}
#[derive(Default)]
struct Uses(Cell<usize>);
impl ApprovalUse for Uses {
    fn consume(&self, _: &Approval) -> Result<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}
#[derive(Default)]
struct Sink(RefCell<Vec<GateRecord>>);
impl GateRecordSink for Sink {
    fn record(&self, record: GateRecord) -> Result<()> {
        self.0.borrow_mut().push(record);
        Ok(())
    }
}

fn declaration(mode: ExecutionMode) -> OperationDeclaration {
    OperationDeclaration {
        operation: "fixture/run".into(),
        subject: Ref::Symbol(Symbol::qualified("fixture", "one")),
        capability: CapabilityName::new("fixture/run"),
        mode,
    }
}
fn effect(cx: &mut Cx, declaration: &OperationDeclaration) -> Effect {
    Effect::new(
        cx.fresh_handle(),
        Symbol::qualified("fixture", "operation"),
        declaration.subject.clone(),
        Ref::Symbol(Symbol::new("input")),
        Ref::Symbol(Symbol::qualified("core", "Any")),
        effect_resume_op_key(),
        effect_abort_op_key(),
    )
    .requiring(declaration.capability.clone())
}

#[test]
fn exact_subject_decision_and_verifier_fail_closed() {
    for (approval, verifier, expected) in [
        (
            Approval {
                id: "a".into(),
                subject: Ref::Symbol(Symbol::qualified("fixture", "wrong")),
                decision: ApprovalDecision::Approve,
            },
            true,
            "subject",
        ),
        (
            Approval {
                id: "a".into(),
                subject: declaration(ExecutionMode::Reviewed).subject,
                decision: ApprovalDecision::Deny,
            },
            true,
            "does not approve",
        ),
        (
            Approval {
                id: "a".into(),
                subject: declaration(ExecutionMode::Reviewed).subject,
                decision: ApprovalDecision::Approve,
            },
            false,
            "expired",
        ),
    ] {
        let mut cx = bare_cx();
        cx.grant_named("fixture/run");
        let d = declaration(ExecutionMode::Reviewed);
        let e = effect(&mut cx, &d);
        let uses = Uses::default();
        let sink = Sink::default();
        let error = guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: Some(&approval),
                verifier: &Verifier(verifier),
                approval_use: &uses,
                sink: &sink,
                sink_failure: SinkFailurePolicy::FailClosed,
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok"))),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected));
        assert_eq!(uses.0.get(), 0);
    }
}

#[test]
fn observation_and_recorded_need_no_approval_but_reviewed_does() {
    for mode in [ExecutionMode::Observation, ExecutionMode::Recorded] {
        let mut cx = bare_cx();
        cx.grant_named("fixture/run");
        let d = declaration(mode);
        let e = effect(&mut cx, &d);
        let uses = Uses::default();
        let sink = Sink::default();
        guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: None,
                verifier: &Verifier(true),
                approval_use: &uses,
                sink: &sink,
                sink_failure: SinkFailurePolicy::FailClosed,
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok"))),
        )
        .unwrap();
        assert_eq!(uses.0.get(), 0);
        assert_eq!(sink.0.borrow()[0].mode, mode);
    }
}

struct DeniedUse;
impl ApprovalUse for DeniedUse {
    fn consume(&self, _: &Approval) -> Result<()> {
        Err(Error::Eval("approval use denied".into()))
    }
}
struct FailingSink;
impl GateRecordSink for FailingSink {
    fn record(&self, _: GateRecord) -> Result<()> {
        Err(Error::Eval("sink failed".into()))
    }
}

#[test]
fn capability_use_performer_and_sink_failures_are_observable() {
    let d = declaration(ExecutionMode::Recorded);
    let mut cx = bare_cx();
    let e = effect(&mut cx, &d);
    let uses = Uses::default();
    let sink = Sink::default();
    assert!(
        guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: None,
                verifier: &Verifier(true),
                approval_use: &uses,
                sink: &sink,
                sink_failure: SinkFailurePolicy::FailClosed
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok")))
        )
        .unwrap_err()
        .to_string()
        .contains("capability")
    );

    let approval = Approval {
        id: "a".into(),
        subject: d.subject.clone(),
        decision: ApprovalDecision::Approve,
    };
    let reviewed = declaration(ExecutionMode::Reviewed);
    let mut cx = bare_cx();
    cx.grant_named("fixture/run");
    let e = effect(&mut cx, &reviewed);
    assert!(
        guard_operation(
            &mut cx,
            &reviewed,
            e,
            GateContext {
                approval: Some(&approval),
                verifier: &Verifier(true),
                approval_use: &DeniedUse,
                sink: &sink,
                sink_failure: SinkFailurePolicy::FailClosed
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok")))
        )
        .unwrap_err()
        .to_string()
        .contains("use denied")
    );

    let mut cx = bare_cx();
    cx.grant_named("fixture/run");
    let e = effect(&mut cx, &d);
    assert!(
        guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: None,
                verifier: &Verifier(true),
                approval_use: &uses,
                sink: &sink,
                sink_failure: SinkFailurePolicy::FailClosed
            },
            |_, _| Err(Error::Eval("performer failed".into()))
        )
        .unwrap_err()
        .to_string()
        .contains("performer failed")
    );

    let mut cx = bare_cx();
    cx.grant_named("fixture/run");
    let e = effect(&mut cx, &d);
    assert!(
        guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: None,
                verifier: &Verifier(true),
                approval_use: &uses,
                sink: &FailingSink,
                sink_failure: SinkFailurePolicy::FailClosed
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok")))
        )
        .unwrap_err()
        .to_string()
        .contains("sink failed")
    );
    let mut cx = bare_cx();
    cx.grant_named("fixture/run");
    let e = effect(&mut cx, &d);
    assert_eq!(
        guard_operation(
            &mut cx,
            &d,
            e,
            GateContext {
                approval: None,
                verifier: &Verifier(true),
                approval_use: &uses,
                sink: &FailingSink,
                sink_failure: SinkFailurePolicy::PreserveResult
            },
            |_, _| Ok(Ref::Symbol(Symbol::new("ok")))
        )
        .unwrap(),
        Ref::Symbol(Symbol::new("ok"))
    );
}
