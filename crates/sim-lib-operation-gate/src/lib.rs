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
mod tests;
