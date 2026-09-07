//! Strict canonical lifecycle journal projection.

use std::collections::BTreeMap;

use sim_kernel::Symbol;
use sim_lib_journal::VerifiedSnapshot;

use crate::{
    OperationAttempt, OperationError, OperationGrant, OperationId, OperationIntent, ReplayPolicy,
    lifecycle::{
        FencedDispatch, LifecycleReceipt, OperationLease, OperationObservation, OperationOutcome,
        OperationStep, PostconditionResponse,
    },
    lifecycle_record::{IdentifiedOutcome, OperationLifecycleRecord},
    lifecycle_wire::{require_payloads, snapshot_datum},
};

pub(super) fn project(
    snapshot: VerifiedSnapshot,
) -> Result<BTreeMap<OperationId, OperationLifecycleRecord>, OperationError> {
    let mut operations = BTreeMap::new();
    for entry in snapshot.entries() {
        if entry.kind == Symbol::qualified("operation", "lifecycle-intent-persisted") {
            require_payloads(entry, 2)?;
            let intent =
                OperationIntent::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let grant = OperationGrant::from_datum(snapshot_datum(&snapshot, &entry.payloads[1])?)?;
            if grant.operation != intent.id {
                return Err(OperationError::GrantMismatch);
            }
            let record = OperationLifecycleRecord {
                intent: intent.clone(),
                grant,
                leases: vec![],
                attempts: vec![],
                dispatches: vec![],
                receipts: vec![],
                observations: vec![],
                outcomes: vec![],
                last_step: OperationStep::IntentPersisted,
            };
            if operations.insert(intent.id().clone(), record).is_some() {
                return Err(OperationError::DuplicateIntent);
            }
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-lease-acquired") {
            require_payloads(entry, 1)?;
            let lease = OperationLease::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let record = operations
                .get_mut(lease.operation())
                .ok_or(OperationError::InvalidTransition("lease before intent"))?;
            if record
                .leases
                .last()
                .is_some_and(|old| old.fence >= lease.fence || old.expires_at > lease.acquired_at)
            {
                return Err(OperationError::InvalidTransition(
                    "overlapping or stale operation lease",
                ));
            }
            record.leases.push(lease);
            record.last_step = OperationStep::LeaseAcquired;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-dispatch-persisted") {
            require_payloads(entry, 2)?;
            let dispatch =
                FencedDispatch::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let attempt =
                OperationAttempt::from_datum(snapshot_datum(&snapshot, &entry.payloads[1])?)?;
            let record = operations
                .get_mut(dispatch.operation())
                .ok_or(OperationError::InvalidTransition("dispatch before intent"))?;
            let lease = record
                .leases
                .last()
                .ok_or(OperationError::InvalidTransition("dispatch before lease"))?;
            if dispatch.grant != record.grant.id
                || dispatch.lease != lease.id
                || dispatch.attempt != attempt.id
                || attempt.operation != record.intent.id
            {
                return Err(OperationError::InvalidTransition(
                    "conflicting fenced dispatch",
                ));
            }
            if record
                .dispatches
                .first()
                .is_some_and(|old| old.performer() != dispatch.performer())
            {
                return Err(OperationError::InvalidTransition(
                    "performer identity changed across recovery",
                ));
            }
            let ordinal = u64::try_from(record.attempts.len())
                .map_err(|_| OperationError::SequenceExhausted)?;
            if attempt.ordinal() != ordinal {
                return Err(OperationError::InvalidTransition(
                    "non-contiguous attempt ordinal",
                ));
            }
            record.attempts.push(attempt);
            record.dispatches.push(dispatch);
            record.last_step = OperationStep::DispatchPersisted;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-receipt-persisted") {
            require_payloads(entry, 1)?;
            let receipt =
                LifecycleReceipt::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let record = operations
                .values_mut()
                .find(|record| {
                    record
                        .dispatches
                        .last()
                        .is_some_and(|dispatch| dispatch.id == receipt.dispatch)
                })
                .ok_or(OperationError::InvalidTransition(
                    "receipt before matching dispatch",
                ))?;
            if record
                .receipts
                .iter()
                .any(|old| old.dispatch == receipt.dispatch)
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate lifecycle receipt",
                ));
            }
            record.receipts.push(receipt);
            record.last_step = OperationStep::ReceiptPersisted;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-observation-persisted") {
            require_payloads(entry, 1)?;
            let observation =
                OperationObservation::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let record = operations.get_mut(observation.operation()).ok_or(
                OperationError::InvalidTransition("observation before intent"),
            )?;
            if observation.dispatch.as_ref()
                != record.dispatches.last().map(|dispatch| &dispatch.id)
            {
                return Err(OperationError::InvalidTransition(
                    "observation dispatch mismatch",
                ));
            }
            if record
                .dispatches
                .last()
                .is_some_and(|dispatch| dispatch.performer() == observation.observer())
            {
                return Err(OperationError::InvalidTransition(
                    "performer authored its own postcondition observation",
                ));
            }
            let matching_receipt = record.dispatches.last().and_then(|dispatch| {
                record
                    .receipts
                    .iter()
                    .rev()
                    .find(|receipt| receipt.dispatch == dispatch.id)
            });
            if observation.receipt.as_ref() != matching_receipt.map(|receipt| &receipt.id) {
                return Err(OperationError::InvalidTransition(
                    "observation receipt mismatch",
                ));
            }
            record.observations.push(observation);
            record.last_step = OperationStep::ObservationPersisted;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-outcome-persisted") {
            require_payloads(entry, 1)?;
            let outcome =
                IdentifiedOutcome::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let record = operations
                .get_mut(&outcome.operation)
                .ok_or(OperationError::InvalidTransition("outcome before intent"))?;
            if record.observations.is_empty() {
                return Err(OperationError::InvalidTransition(
                    "outcome before observation",
                ));
            }
            let observation = record.observations.last().expect("checked non-empty");
            match &outcome.outcome {
                OperationOutcome::AlreadyTrue { evidence }
                    if !matches!(
                        observation.response(),
                        PostconditionResponse::Satisfied { .. }
                    ) || !record.dispatches.is_empty()
                        || evidence != observation.evidence() =>
                {
                    return Err(OperationError::InvalidTransition(
                        "invalid already-true outcome",
                    ));
                }
                OperationOutcome::Verified { evidence }
                    if !matches!(
                        observation.response(),
                        PostconditionResponse::Satisfied { .. }
                    ) || record.dispatches.is_empty()
                        || evidence != observation.evidence() =>
                {
                    return Err(OperationError::InvalidTransition(
                        "invalid verified outcome",
                    ));
                }
                OperationOutcome::Diverged { observed, expected }
                    if expected != record.intent.intended_result()
                        || !matches!(
                            observation.response(),
                            PostconditionResponse::NotSatisfied {
                                observed: observation_value,
                                ..
                            } if observation_value == observed
                        ) =>
                {
                    return Err(OperationError::InvalidTransition(
                        "diverged expected value mismatch",
                    ));
                }
                OperationOutcome::Uncertain { last_durable_step }
                    if *last_durable_step != record.last_step()
                        || !matches!(
                            observation.response(),
                            PostconditionResponse::Unavailable { .. }
                                | PostconditionResponse::Disputed { .. }
                        ) && !matches!(
                            observation.response(),
                            PostconditionResponse::NotSatisfied { .. }
                        ) =>
                {
                    return Err(OperationError::InvalidTransition("uncertain step mismatch"));
                }
                OperationOutcome::Uncertain { .. }
                    if matches!(
                        observation.response(),
                        PostconditionResponse::NotSatisfied { .. }
                    ) && (record.intent.replay_policy() != ReplayPolicy::Idempotent
                        || !record
                            .leases
                            .last()
                            .is_some_and(|lease| lease.is_live_at(observation.observed_at()))) =>
                {
                    return Err(OperationError::InvalidTransition(
                        "uncertain outcome lacks a live idempotent lease",
                    ));
                }
                _ => {}
            }
            record.outcomes.push(outcome);
            record.last_step = OperationStep::OutcomePersisted;
        }
    }
    Ok(operations)
}
