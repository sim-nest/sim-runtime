//! Canonical semantic encoding for lifecycle records.

use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::{JournalEntry, VerifiedSnapshot};

use crate::{
    OperationError, OperationId,
    lifecycle::{
        DISPATCH_TAG, EvidenceSetId, FencedDispatchId, LEASE_TAG, LifecycleReceiptId,
        OBSERVATION_TAG, OUTCOME_TAG, OperationLeaseId, OperationOutcome, OperationStep,
        PostconditionRequest, PostconditionResponse, RECEIPT_TAG,
    },
    operation_wire::{field, id_datum, id_from_datum, node, node_fields},
};

pub(super) fn lease_datum(
    operation: &OperationId,
    holder: &Datum,
    fence: u64,
    acquired_at: u64,
    expires_at: u64,
) -> Datum {
    node(
        LEASE_TAG,
        vec![
            ("operation", id_datum(operation.content_id())),
            ("holder", holder.clone()),
            ("fence", u64_datum(fence)),
            ("acquired-at", u64_datum(acquired_at)),
            ("expires-at", u64_datum(expires_at)),
        ],
    )
}
pub(super) fn fenced_dispatch_datum(
    operation: &OperationId,
    grant: &crate::OperationGrantId,
    attempt: &crate::OperationAttemptId,
    lease: &OperationLeaseId,
    performer: &Datum,
) -> Datum {
    node(
        DISPATCH_TAG,
        vec![
            ("operation", id_datum(operation.content_id())),
            ("grant", id_datum(grant.content_id())),
            ("attempt", id_datum(attempt.content_id())),
            ("lease", id_datum(lease.content_id())),
            ("performer", performer.clone()),
        ],
    )
}
pub(super) fn lifecycle_receipt_datum(dispatch: &FencedDispatchId, raw: &Datum) -> Datum {
    node(
        RECEIPT_TAG,
        vec![
            ("dispatch", id_datum(dispatch.content_id())),
            ("raw", raw.clone()),
        ],
    )
}
pub(super) fn observation_base_datum(
    request: &PostconditionRequest,
    observer: &Datum,
    response: &PostconditionResponse,
) -> Datum {
    node(
        "observation-evidence-v1",
        vec![
            ("operation", id_datum(request.operation.content_id())),
            ("observer", observer.clone()),
            ("response", response_datum(response)),
            (
                "dispatch",
                optional_id_datum(request.dispatch.as_ref().map(FencedDispatchId::content_id)),
            ),
            (
                "receipt",
                optional_id_datum(request.receipt.as_ref().map(LifecycleReceiptId::content_id)),
            ),
            ("last-durable-step", request.last_durable_step.datum()),
            ("observed-at", u64_datum(request.observed_at)),
        ],
    )
}
pub(super) fn observation_datum(
    request: &PostconditionRequest,
    observer: &Datum,
    response: &PostconditionResponse,
    evidence: &EvidenceSetId,
) -> Datum {
    node(
        OBSERVATION_TAG,
        vec![
            ("operation", id_datum(request.operation.content_id())),
            ("observer", observer.clone()),
            ("response", response_datum(response)),
            (
                "dispatch",
                optional_id_datum(request.dispatch.as_ref().map(FencedDispatchId::content_id)),
            ),
            (
                "receipt",
                optional_id_datum(request.receipt.as_ref().map(LifecycleReceiptId::content_id)),
            ),
            ("last-durable-step", request.last_durable_step.datum()),
            ("observed-at", u64_datum(request.observed_at)),
            ("evidence", id_datum(evidence.content_id())),
        ],
    )
}
pub(super) fn response_datum(response: &PostconditionResponse) -> Datum {
    match response {
        PostconditionResponse::Satisfied { observed, evidence } => node(
            "observation-satisfied-v1",
            vec![
                ("observed", observed.clone()),
                ("evidence", evidence.clone()),
            ],
        ),
        PostconditionResponse::NotSatisfied { observed, evidence } => node(
            "observation-not-satisfied-v1",
            vec![
                ("observed", observed.clone()),
                ("evidence", evidence.clone()),
            ],
        ),
        PostconditionResponse::Unavailable { reason } => node(
            "observation-unavailable-v1",
            vec![("reason", reason.clone())],
        ),
        PostconditionResponse::Disputed { first, second } => node(
            "observation-disputed-v1",
            vec![("first", first.clone()), ("second", second.clone())],
        ),
    }
}
pub(super) fn response_from_datum(datum: &Datum) -> Result<PostconditionResponse, OperationError> {
    if let Ok(fields) = node_fields(datum, "observation-satisfied-v1", 2) {
        return Ok(PostconditionResponse::Satisfied {
            observed: field(fields, "observed")?.clone(),
            evidence: field(fields, "evidence")?.clone(),
        });
    }
    if let Ok(fields) = node_fields(datum, "observation-not-satisfied-v1", 2) {
        return Ok(PostconditionResponse::NotSatisfied {
            observed: field(fields, "observed")?.clone(),
            evidence: field(fields, "evidence")?.clone(),
        });
    }
    if let Ok(fields) = node_fields(datum, "observation-unavailable-v1", 1) {
        return Ok(PostconditionResponse::Unavailable {
            reason: field(fields, "reason")?.clone(),
        });
    }
    if let Ok(fields) = node_fields(datum, "observation-disputed-v1", 2) {
        return Ok(PostconditionResponse::Disputed {
            first: field(fields, "first")?.clone(),
            second: field(fields, "second")?.clone(),
        });
    }
    Err(OperationError::NonCanonical("postcondition response"))
}
pub(super) fn outcome_datum(operation: &OperationId, outcome: &OperationOutcome) -> Datum {
    node(
        OUTCOME_TAG,
        vec![
            ("operation", id_datum(operation.content_id())),
            ("value", outcome_value_datum(outcome)),
            ("schema", Datum::String("operation/outcome-v1".into())),
        ],
    )
}
fn outcome_value_datum(outcome: &OperationOutcome) -> Datum {
    match outcome {
        OperationOutcome::AlreadyTrue { evidence } => node(
            "already-true-v1",
            vec![("evidence", id_datum(evidence.content_id()))],
        ),
        OperationOutcome::Verified { evidence } => node(
            "verified-v1",
            vec![("evidence", id_datum(evidence.content_id()))],
        ),
        OperationOutcome::Diverged { observed, expected } => node(
            "diverged-v1",
            vec![
                ("observed", observed.clone()),
                ("expected", expected.clone()),
            ],
        ),
        OperationOutcome::Uncertain { last_durable_step } => node(
            "uncertain-v1",
            vec![("last-durable-step", last_durable_step.datum())],
        ),
    }
}
pub(super) fn outcome_from_datum(datum: &Datum) -> Result<OperationOutcome, OperationError> {
    if let Ok(fields) = node_fields(datum, "already-true-v1", 1) {
        return Ok(OperationOutcome::AlreadyTrue {
            evidence: EvidenceSetId(id_from_datum(field(fields, "evidence")?)?),
        });
    }
    if let Ok(fields) = node_fields(datum, "verified-v1", 1) {
        return Ok(OperationOutcome::Verified {
            evidence: EvidenceSetId(id_from_datum(field(fields, "evidence")?)?),
        });
    }
    if let Ok(fields) = node_fields(datum, "diverged-v1", 2) {
        return Ok(OperationOutcome::Diverged {
            observed: field(fields, "observed")?.clone(),
            expected: field(fields, "expected")?.clone(),
        });
    }
    if let Ok(fields) = node_fields(datum, "uncertain-v1", 1) {
        return Ok(OperationOutcome::Uncertain {
            last_durable_step: OperationStep::from_datum(field(fields, "last-durable-step")?)?,
        });
    }
    Err(OperationError::NonCanonical("operation outcome value"))
}
pub(super) fn optional_id_datum(id: Option<&ContentId>) -> Datum {
    id.map_or(Datum::Nil, id_datum)
}
pub(super) fn optional_id(datum: &Datum) -> Result<Option<ContentId>, OperationError> {
    if *datum == Datum::Nil {
        Ok(None)
    } else {
        id_from_datum(datum).map(Some)
    }
}
pub(super) fn u64_datum(value: u64) -> Datum {
    Datum::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}
pub(super) fn snapshot_datum<'a>(
    snapshot: &'a VerifiedSnapshot,
    id: &ContentId,
) -> Result<&'a Datum, OperationError> {
    snapshot
        .datum(id)
        .ok_or(OperationError::NonCanonical("missing lifecycle payload"))
}
pub(super) fn require_payloads(entry: &JournalEntry, count: usize) -> Result<(), OperationError> {
    if entry.payloads.len() == count {
        Ok(())
    } else {
        Err(OperationError::NonCanonical("lifecycle event payloads"))
    }
}
