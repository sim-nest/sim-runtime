//! Declaration-driven gate for capability-scoped operations.
//!
//! The gate contains no domain policy: callers provide a manifest declaration,
//! exact approval verifier/use adapters, a record sink, and the performer.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use sim_kernel::{
    CapabilityName, Cx, Error, Ref, Result,
    effect::{Effect, resolve_effect},
};

/// Policy label for an operation. None implies reversibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Read or observation whose execution is still an effect.
    Observation,
    /// Effect recorded for audit and replay without review.
    Recorded,
    /// Effect requiring an exact approval before its first performance.
    Reviewed,
}

/// Canonical operation declaration supplied by a domain manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationDeclaration {
    /// Exact operation identity.
    pub operation: String,
    /// Exact approval subject.
    pub subject: Ref,
    /// Required capability.
    pub capability: CapabilityName,
    /// Execution policy label.
    pub mode: ExecutionMode,
}

/// Approval presented for a reviewed operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// Stable approval identity.
    pub id: String,
    /// Exact subject this approval authorizes.
    pub subject: Ref,
    /// Decision asserted by the approver.
    pub decision: ApprovalDecision,
}

/// Explicit approval decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Permit the exact subject.
    Approve,
    /// Refuse the exact subject.
    Deny,
}

/// Validates approval authenticity and validity without consuming it.
pub trait ApprovalVerifier {
    /// Reject invalid, expired, or otherwise unusable approval evidence.
    fn verify(&self, approval: &Approval) -> Result<()>;
}

/// Atomically consumes a verified approval once.
pub trait ApprovalUse {
    /// Consume `approval`, rejecting reuse or policy denial.
    fn consume(&self, approval: &Approval) -> Result<()>;
}

/// Audit record emitted after successful first performance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRecord {
    /// Operation identity.
    pub operation: String,
    /// Exact subject.
    pub subject: Ref,
    /// Required capability.
    pub capability: CapabilityName,
    /// Applied mode.
    pub mode: ExecutionMode,
    /// Consumed approval id, if reviewed.
    pub approval: Option<String>,
    /// Result returned by the performer.
    pub result: Ref,
}

/// Receives gate records.
pub trait GateRecordSink {
    /// Persist one successful first-performance record.
    fn record(&self, record: GateRecord) -> Result<()>;
}

/// Policy for a record-sink failure after the operation performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkFailurePolicy {
    /// Fail closed and surface the sink error.
    FailClosed,
    /// Keep the successful operation result despite an unavailable sink.
    PreserveResult,
}

/// Dependencies used to guard one operation.
pub struct GateContext<'a> {
    /// Optional approval for a reviewed operation.
    pub approval: Option<&'a Approval>,
    /// Approval verifier.
    pub verifier: &'a dyn ApprovalVerifier,
    /// Atomic approval-use adapter.
    pub approval_use: &'a dyn ApprovalUse,
    /// Audit sink.
    pub sink: &'a dyn GateRecordSink,
    /// Explicit sink failure policy.
    pub sink_failure: SinkFailurePolicy,
}

/// Guard and resolve an effect, returning the kernel's result reference directly.
///
/// Approval verification, atomic use, performance, and record emission occur
/// only in the `resolve_effect` performer. Cassette replay therefore repeats
/// none of them.
pub fn guard_operation<F>(
    cx: &mut Cx,
    declaration: &OperationDeclaration,
    effect: Effect,
    gate: GateContext<'_>,
    perform: F,
) -> Result<Ref>
where
    F: FnOnce(&mut Cx, &Effect) -> Result<Ref>,
{
    if !effect
        .requires
        .iter()
        .any(|capability| capability == &declaration.capability)
    {
        return Err(Error::Eval(format!(
            "operation {} effect omits declared capability {}",
            declaration.operation,
            declaration.capability.as_str()
        )));
    }
    resolve_effect(cx, effect, |cx, effect| {
        let approval_id = match declaration.mode {
            ExecutionMode::Observation | ExecutionMode::Recorded => None,
            ExecutionMode::Reviewed => {
                let approval = gate.approval.ok_or_else(|| {
                    Error::Eval(format!(
                        "operation {} requires approval",
                        declaration.operation
                    ))
                })?;
                if approval.subject != declaration.subject {
                    return Err(Error::Eval(format!(
                        "approval {} subject does not match operation {} subject",
                        approval.id, declaration.operation
                    )));
                }
                if approval.decision != ApprovalDecision::Approve {
                    return Err(Error::Eval(format!(
                        "approval {} does not approve",
                        approval.id
                    )));
                }
                gate.verifier.verify(approval)?;
                gate.approval_use.consume(approval)?;
                Some(approval.id.clone())
            }
        };
        let result = perform(cx, effect)?;
        let record = GateRecord {
            operation: declaration.operation.clone(),
            subject: declaration.subject.clone(),
            capability: declaration.capability.clone(),
            mode: declaration.mode,
            approval: approval_id,
            result: result.clone(),
        };
        match gate.sink.record(record) {
            Ok(()) => Ok(result),
            Err(_) if gate.sink_failure == SinkFailurePolicy::PreserveResult => Ok(result),
            Err(error) => Err(error),
        }
    })
}

#[cfg(test)]
mod tests {
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
    fn effect(_cx: &mut Cx, declaration: &OperationDeclaration) -> Effect {
        Effect::new(
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
    impl ApprovalUse for DeniedUse { fn consume(&self, _: &Approval) -> Result<()> { Err(Error::Eval("approval use denied".into())) } }
    struct FailingSink;
    impl GateRecordSink for FailingSink { fn record(&self, _: GateRecord) -> Result<()> { Err(Error::Eval("sink failed".into())) } }

    #[test]
    fn capability_use_performer_and_sink_failures_are_observable() {
        let d = declaration(ExecutionMode::Recorded);
        let mut cx = bare_cx(); let e = effect(&mut cx, &d); let uses = Uses::default(); let sink = Sink::default();
        assert!(guard_operation(&mut cx, &d, e, GateContext { approval: None, verifier: &Verifier(true), approval_use: &uses, sink: &sink, sink_failure: SinkFailurePolicy::FailClosed }, |_, _| Ok(Ref::Symbol(Symbol::new("ok")))).unwrap_err().to_string().contains("capability"));

        let approval = Approval { id: "a".into(), subject: d.subject.clone(), decision: ApprovalDecision::Approve };
        let reviewed = declaration(ExecutionMode::Reviewed); let mut cx = bare_cx(); cx.grant_named("fixture/run"); let e = effect(&mut cx, &reviewed);
        assert!(guard_operation(&mut cx, &reviewed, e, GateContext { approval: Some(&approval), verifier: &Verifier(true), approval_use: &DeniedUse, sink: &sink, sink_failure: SinkFailurePolicy::FailClosed }, |_, _| Ok(Ref::Symbol(Symbol::new("ok")))).unwrap_err().to_string().contains("use denied"));

        let mut cx = bare_cx(); cx.grant_named("fixture/run"); let e = effect(&mut cx, &d);
        assert!(guard_operation(&mut cx, &d, e, GateContext { approval: None, verifier: &Verifier(true), approval_use: &uses, sink: &sink, sink_failure: SinkFailurePolicy::FailClosed }, |_, _| Err(Error::Eval("performer failed".into()))).unwrap_err().to_string().contains("performer failed"));

        let mut cx = bare_cx(); cx.grant_named("fixture/run"); let e = effect(&mut cx, &d);
        assert!(guard_operation(&mut cx, &d, e, GateContext { approval: None, verifier: &Verifier(true), approval_use: &uses, sink: &FailingSink, sink_failure: SinkFailurePolicy::FailClosed }, |_, _| Ok(Ref::Symbol(Symbol::new("ok")))).unwrap_err().to_string().contains("sink failed"));
        let mut cx = bare_cx(); cx.grant_named("fixture/run"); let e = effect(&mut cx, &d);
        assert_eq!(guard_operation(&mut cx, &d, e, GateContext { approval: None, verifier: &Verifier(true), approval_use: &uses, sink: &FailingSink, sink_failure: SinkFailurePolicy::PreserveResult }, |_, _| Ok(Ref::Symbol(Symbol::new("ok")))).unwrap(), Ref::Symbol(Symbol::new("ok")));
    }
}
