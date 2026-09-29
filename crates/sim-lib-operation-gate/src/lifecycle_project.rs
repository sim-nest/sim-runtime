//! Strict canonical lifecycle journal projection.

use std::collections::BTreeMap;

use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::{JournalEntry, JournalObject, VerifiedSnapshot};

use crate::{
    LifecyclePreparation, OperationAttempt, OperationError, OperationGrant, OperationId,
    OperationIntent, ReplayPolicy,
    lifecycle::{
        FencedDispatch, LifecycleReceipt, OperationLease, OperationObservation, OperationOutcome,
        OperationStep, PostconditionResponse,
    },
    lifecycle_project_validate::{validate_closed_snapshot, validate_next_entry},
    lifecycle_record::{IdentifiedOutcome, OperationLifecycleRecord},
    lifecycle_wire::{require_payloads, snapshot_datum},
};

pub(super) fn project(
    snapshot: VerifiedSnapshot,
) -> Result<BTreeMap<OperationId, OperationLifecycleRecord>, OperationError> {
    project_entries(snapshot.entries().iter(), |id| {
        snapshot_datum(&snapshot, id)
    })
}

/// Projects every lifecycle from one exact, closed, verified journal snapshot.
///
/// Unlike the internal mixed-journal reducer, this public evidence boundary
/// refuses unknown entry kinds and unreferenced objects. It performs no backend
/// read, acquires no writer, and grants no operation or replay authority.
pub fn project_verified_lifecycle_records(
    snapshot: &VerifiedSnapshot,
) -> Result<BTreeMap<OperationId, OperationLifecycleRecord>, OperationError> {
    validate_closed_snapshot(snapshot)?;
    project(snapshot.clone())
}

/// Projects an exact verified prefix plus one prospective, uncommitted entry.
///
/// The extension must be the unique next entry over the supplied head. Every
/// supplied object must be content-valid and referenced by that entry. This is
/// a read-only validation surface for crash-cut evidence; it does not append or
/// otherwise mutate the journal.
pub fn project_verified_lifecycle_extension(
    snapshot: &VerifiedSnapshot,
    entry: &JournalEntry,
    objects: &[JournalObject],
) -> Result<BTreeMap<OperationId, OperationLifecycleRecord>, OperationError> {
    validate_closed_snapshot(snapshot)?;
    validate_next_entry(snapshot, entry, objects)?;
    project_entries(
        snapshot.entries().iter().chain(std::iter::once(entry)),
        |id| match objects.iter().find(|object| &object.id == id) {
            Some(object) => Ok(object.datum()),
            None => snapshot_datum(snapshot, id),
        },
    )
}

/// Checks a proposed transition against the exact head used by its later CAS.
/// Admission and replay share this reducer; a concurrent append cannot silently
/// rebase a decision onto a state that did not authorize it.
pub(super) fn validate_extension(
    snapshot: &VerifiedSnapshot,
    entry: &JournalEntry,
    objects: &[JournalObject],
) -> Result<(), OperationError> {
    project_verified_lifecycle_extension(snapshot, entry, objects)?;
    Ok(())
}

fn project_entries<'a>(
    entries: impl Iterator<Item = &'a JournalEntry>,
    datum: impl Fn(&ContentId) -> Result<&'a Datum, OperationError>,
) -> Result<BTreeMap<OperationId, OperationLifecycleRecord>, OperationError> {
    let mut operations = BTreeMap::new();
    for entry in entries {
        if entry.kind == Symbol::qualified("operation", "lifecycle-intent-persisted") {
            require_payloads(entry, 2)?;
            let intent = OperationIntent::from_datum(datum(&entry.payloads[0])?)?;
            let grant = OperationGrant::from_datum(datum(&entry.payloads[1])?)?;
            if grant.operation != intent.id {
                return Err(OperationError::GrantMismatch);
            }
            let record = OperationLifecycleRecord {
                intent: intent.clone(),
                grant,
                leases: vec![],
                attempts: vec![],
                dispatches: vec![],
                preparations: vec![],
                reservations: vec![],
                releases: vec![],
                cancellation: None,
                receipts: vec![],
                observations: vec![],
                outcome: None,
                last_step: OperationStep::IntentPersisted,
                generation: entry.sequence,
                observation_generation: None,
            };
            if operations.insert(intent.id().clone(), record).is_some() {
                return Err(OperationError::DuplicateIntent);
            }
        } else if entry.kind
            == Symbol::qualified("operation", "lifecycle-cancellation-intent-persisted")
        {
            require_payloads(entry, 1)?;
            let cancellation =
                crate::LifecycleCancellation::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations.get_mut(cancellation.operation()).ok_or(
                OperationError::InvalidTransition("cancellation before intent"),
            )?;
            require_unfinished(record)?;
            if record.cancellation.is_some() || record.grant.id() != cancellation.grant() {
                return Err(OperationError::InvalidTransition(
                    "duplicate or unbound cancellation",
                ));
            }
            record.cancellation = Some(cancellation);
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-lease-acquired") {
            require_payloads(entry, 1)?;
            let lease = OperationLease::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations
                .get_mut(lease.operation())
                .ok_or(OperationError::InvalidTransition("lease before intent"))?;
            require_unfinished(record)?;
            if record.cancellation.is_some() {
                return Err(OperationError::Cancelled);
            }
            if !record.reservations.is_empty() || !record.preparations.is_empty() {
                return Err(OperationError::InvalidTransition(
                    "new attempt lease before reserved resource disposition",
                ));
            }
            if record.leases.last().is_some_and(|old| {
                !crate::operation_wire::same_optional_datum(old.clock(), lease.clock())
                    || old.fence >= lease.fence
                    || old.expires_at > lease.acquired_at
            }) {
                return Err(OperationError::InvalidTransition(
                    "overlapping or stale operation lease",
                ));
            }
            // A second lease over an existing dispatch is a retry. ExactlyOnce
            // never retries. Idempotent may retry only after an observation
            // made against the exact prior dispatch independently confirmed a
            // negative postcondition while the prior lease was already dead --
            // matching the engine's own precondition for falling through to a
            // fresh attempt, enforced here so no canonical extension can skip
            // it. Unresolved custody (reservation/preparation) and durable
            // cancellation are already refused above for every lease.
            if !record.dispatches.is_empty() {
                if record.intent.replay_policy() != ReplayPolicy::Idempotent {
                    return Err(OperationError::InvalidTransition(
                        "replay policy forbids a retry lease",
                    ));
                }
                let observation =
                    record
                        .observations
                        .last()
                        .ok_or(OperationError::InvalidTransition(
                            "retry lease requires a prior observation",
                        ))?;
                // Freshness: nothing has touched this record since the
                // observation was itself recorded. record.generation is
                // stamped on every state-advancing entry; observation_generation
                // captures its value when the current observation was
                // recorded. Equal means the observation is still the single
                // most recent fact about this record -- not merely that it
                // names the record's current last dispatch (a new lease
                // without a new dispatch would leave that check blind).
                // Requires positive proof the prior lease's window fully
                // elapsed (same clock domain, observed_at >= expires_at) --
                // not merely that the observation's tick numerically fails
                // is_live_at, which is equally true for an observation from
                // an unrelated clock, or one that precedes acquisition.
                if record.observation_generation != Some(record.generation)
                    || !matches!(
                        observation.response(),
                        PostconditionResponse::NotSatisfied { .. }
                    )
                    || !record.leases.last().is_some_and(|old| {
                        old.expired_as_of(observation.clock(), observation.observed_at())
                    })
                {
                    return Err(OperationError::InvalidTransition(
                        "retry lease requires a fresh confirmed negative observation proving the prior lease expired",
                    ));
                }
            }
            record.leases.push(lease);
            record.last_step = OperationStep::LeaseAcquired;
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-dispatch-persisted") {
            require_payloads(entry, 2)?;
            let dispatch = FencedDispatch::from_datum(datum(&entry.payloads[0])?)?;
            let attempt = OperationAttempt::from_datum(datum(&entry.payloads[1])?)?;
            let record = operations
                .get_mut(dispatch.operation())
                .ok_or(OperationError::InvalidTransition("dispatch before intent"))?;
            require_unfinished(record)?;
            if record.cancellation.is_some() {
                return Err(OperationError::Cancelled);
            }
            if record.last_step != OperationStep::LeaseAcquired
                || record
                    .dispatches
                    .iter()
                    .any(|old| old.lease() == dispatch.lease())
            {
                return Err(OperationError::InvalidTransition(
                    "dispatch requires a fresh unused attempt lease",
                ));
            }
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
            if record.dispatches.first().is_some_and(|old| {
                !crate::operation_wire::same_datum(old.performer(), dispatch.performer())
            }) {
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
            record.generation = entry.sequence;
        } else if entry.kind
            == Symbol::qualified("operation", "lifecycle-reservation-intent-persisted")
        {
            require_payloads(entry, 1)?;
            let reservation = crate::LifecycleReservation::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations
                .values_mut()
                .find(|record| {
                    record
                        .dispatches
                        .last()
                        .is_some_and(|dispatch| dispatch.id() == reservation.dispatch())
                })
                .ok_or(OperationError::InvalidTransition(
                    "reservation before matching dispatch",
                ))?;
            require_unfinished(record)?;
            if record.cancellation.is_some() {
                return Err(OperationError::Cancelled);
            }
            if record.last_step != OperationStep::DispatchPersisted
                || record
                    .reservations
                    .iter()
                    .any(|old| old.dispatch() == reservation.dispatch())
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate or late reservation intent",
                ));
            }
            record.reservations.push(reservation);
            record.last_step = OperationStep::ReservationIntentPersisted;
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-preparation-persisted") {
            require_payloads(entry, 1)?;
            let preparation = LifecyclePreparation::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations
                .values_mut()
                .find(|record| {
                    record
                        .dispatches
                        .last()
                        .is_some_and(|dispatch| dispatch.id() == preparation.dispatch())
                })
                .ok_or(OperationError::InvalidTransition(
                    "preparation before matching dispatch",
                ))?;
            require_unfinished(record)?;
            if !matches!(
                record.last_step,
                OperationStep::DispatchPersisted | OperationStep::ReservationIntentPersisted
            ) || record
                .preparations
                .iter()
                .any(|old| old.dispatch() == preparation.dispatch())
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate or late preparation",
                ));
            }
            let reservation = record
                .reservations
                .iter()
                .find(|value| value.dispatch() == preparation.dispatch());
            if preparation.reservation() != reservation.map(|value| value.id()) {
                return Err(OperationError::InvalidTransition(
                    "preparation reservation mismatch",
                ));
            }
            record.preparations.push(preparation);
            record.last_step = OperationStep::PreparationPersisted;
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-release-intent-persisted")
        {
            require_payloads(entry, 1)?;
            let release = crate::LifecycleRelease::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations
                .values_mut()
                .find(|record| {
                    record
                        .dispatches
                        .last()
                        .is_some_and(|dispatch| dispatch.id() == release.dispatch())
                })
                .ok_or(OperationError::InvalidTransition(
                    "release before matching dispatch",
                ))?;
            require_unfinished(record)?;
            if record.cancellation.is_some() {
                return Err(OperationError::Cancelled);
            }
            if record.last_step != OperationStep::PreparationPersisted
                || record
                    .preparations
                    .last()
                    .is_none_or(|preparation| preparation.id() != release.preparation())
                || record
                    .leases
                    .last()
                    .is_none_or(|lease| lease.id() != release.lease() || lease.clock().is_none())
                || record
                    .releases
                    .iter()
                    .any(|old| old.dispatch() == release.dispatch())
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate, late or unbound release intent",
                ));
            }
            record.releases.push(release);
            record.last_step = OperationStep::ReleaseIntentPersisted;
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-receipt-persisted") {
            require_payloads(entry, 1)?;
            let receipt = LifecycleReceipt::from_datum(datum(&entry.payloads[0])?)?;
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
            require_unfinished(record)?;
            if record
                .receipts
                .iter()
                .any(|old| old.dispatch == receipt.dispatch)
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate lifecycle receipt",
                ));
            }
            let release = record
                .releases
                .iter()
                .find(|value| value.dispatch() == receipt.dispatch());
            if receipt.release() != release.map(|value| value.id()) {
                return Err(OperationError::InvalidTransition(
                    "receipt release mismatch",
                ));
            }
            record.receipts.push(receipt);
            record.last_step = OperationStep::ReceiptPersisted;
            record.generation = entry.sequence;
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-observation-persisted") {
            require_payloads(entry, 1)?;
            let observation = OperationObservation::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations.get_mut(observation.operation()).ok_or(
                OperationError::InvalidTransition("observation before intent"),
            )?;
            require_unfinished(record)?;
            // Re-appending an observation already present (by id -- the
            // exact same content) is a replay, not a new fact: without
            // this, a canonical writer could re-emit the record's own
            // existing observation as a "new" entry after a retry lease,
            // which would re-stamp generation/observation_generation to
            // the new entry's sequence and make genuinely stale evidence
            // (one that predates the lease) look fresh again.
            if record
                .observations
                .iter()
                .any(|old| old.id() == observation.id())
            {
                return Err(OperationError::InvalidTransition(
                    "duplicate observation replay",
                ));
            }
            if observation.dispatch.as_ref()
                != record.dispatches.last().map(|dispatch| &dispatch.id)
            {
                return Err(OperationError::InvalidTransition(
                    "observation dispatch mismatch",
                ));
            }
            if record.dispatches.last().is_some_and(|dispatch| {
                crate::operation_wire::same_datum(dispatch.performer(), observation.observer())
            }) {
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
            let reservation = record.dispatches.last().and_then(|dispatch| {
                record
                    .reservations
                    .iter()
                    .find(|value| value.dispatch() == dispatch.id())
            });
            if observation.reservation() != reservation.map(|value| value.id()) {
                return Err(OperationError::InvalidTransition(
                    "observation reservation mismatch",
                ));
            }
            let preparation = record.dispatches.last().and_then(|dispatch| {
                record
                    .preparations
                    .iter()
                    .find(|value| value.dispatch() == dispatch.id())
            });
            if observation.preparation.as_ref() != preparation.map(|value| value.id()) {
                return Err(OperationError::InvalidTransition(
                    "observation preparation mismatch",
                ));
            }
            let release = record.dispatches.last().and_then(|dispatch| {
                record
                    .releases
                    .iter()
                    .find(|value| value.dispatch() == dispatch.id())
            });
            if observation.release() != release.map(|value| value.id()) {
                return Err(OperationError::InvalidTransition(
                    "observation release mismatch",
                ));
            }
            if release.is_some() && observation.clock().is_none() {
                return Err(OperationError::InvalidTransition(
                    "released observation lacks clock domain",
                ));
            }
            if let Some(release) = release {
                let lease = record
                    .leases
                    .iter()
                    .find(|lease| lease.id() == release.lease())
                    .ok_or(OperationError::InvalidTransition("release lease missing"))?;
                if !crate::operation_wire::same_optional_datum(observation.clock(), lease.clock())
                    || observation.observed_at() < lease.acquired_at()
                {
                    return Err(OperationError::InvalidTransition(
                        "released observation clock differs or precedes acquisition",
                    ));
                }
            }
            if (reservation.is_some() || preparation.is_some())
                && release.is_none()
                && matches!(
                    observation.response(),
                    PostconditionResponse::Satisfied { .. }
                )
            {
                return Err(OperationError::InvalidTransition(
                    "reserved or prepared success without release intent",
                ));
            }
            record.observations.push(observation);
            record.last_step = OperationStep::ObservationPersisted;
            record.generation = entry.sequence;
            record.observation_generation = Some(entry.sequence);
        } else if entry.kind == Symbol::qualified("operation", "lifecycle-outcome-persisted") {
            require_payloads(entry, 1)?;
            let outcome = IdentifiedOutcome::from_datum(datum(&entry.payloads[0])?)?;
            let record = operations
                .get_mut(&outcome.operation)
                .ok_or(OperationError::InvalidTransition("outcome before intent"))?;
            require_unfinished(record)?;
            if record.observations.is_empty() {
                return Err(OperationError::InvalidTransition(
                    "outcome before observation",
                ));
            }
            let observation = record.observations.last().expect("checked non-empty");
            if outcome.observation != *observation.id() {
                return Err(OperationError::InvalidTransition(
                    "outcome observation mismatch",
                ));
            }
            // Being the record's last observation is not enough: a later
            // lease, dispatch, reservation, preparation, or release (a
            // retry, or custody progress) can be admitted without ever
            // being observed, leaving a stale earlier observation as the
            // record's only one. record.generation is stamped on every
            // state-advancing entry; observation_generation captures its
            // value when the current observation was recorded. Equal means
            // no state-advancing transition happened after it -- checked
            // once here, generically, rather than by comparing individual
            // derived facts (dispatch identity) against each other.
            if record.observation_generation != Some(record.generation) {
                return Err(OperationError::InvalidTransition(
                    "outcome observation is stale: a later transition has since been admitted",
                ));
            }
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
                    if !crate::operation_wire::same_datum(
                        expected,
                        record.intent.intended_result(),
                    ) || !matches!(
                        observation.response(),
                        PostconditionResponse::NotSatisfied {
                            observed: observation_value,
                            ..
                        } if crate::operation_wire::same_datum(observation_value, observed)
                    )
                        // A negative postcondition is not a resource-disposition
                        // proof: unresolved custody (a reservation, a
                        // preparation) or a durable cancellation makes a
                        // sealed Diverged invalid regardless of value
                        // agreement, matching the engine's own rule for
                        // producing this outcome in the first place. This is
                        // enforced again here, independent of the engine, so
                        // no canonical extension can claim it either.
                        || record.cancellation.is_some()
                        || !record.reservations.is_empty()
                        || !record.preparations.is_empty()
                        // Diverged requires an actual attempt: a bare negative
                        // preflight observation with no dispatch is not proof
                        // the intended effect was ever attempted and failed.
                        || record.dispatches.is_empty()
                        // Diverged requires positive proof the lease's
                        // window fully elapsed (same clock domain,
                        // observed_at >= expires_at) -- a live lease means
                        // the original performer may still complete
                        // regardless of replay policy, and an observation
                        // that merely fails is_live_at numerically (an
                        // unrelated clock, or one that precedes
                        // acquisition) proves nothing either way.
                        || !record.leases.last().is_some_and(|lease| {
                            lease.expired_as_of(observation.clock(), observation.observed_at())
                        }) =>
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
                    ) && record.reservations.is_empty()
                        && record.preparations.is_empty()
                        && record.cancellation.is_none()
                        && (record.intent.replay_policy() != ReplayPolicy::Idempotent
                            || !record.leases.last().is_some_and(|lease| {
                                crate::operation_wire::same_optional_datum(
                                    lease.clock(),
                                    observation.clock(),
                                ) && lease.is_live_at(observation.observed_at())
                            })) =>
                {
                    return Err(OperationError::InvalidTransition(
                        "uncertain outcome lacks a live idempotent lease",
                    ));
                }
                _ => {}
            }
            record.outcome = Some(outcome);
            record.last_step = OperationStep::OutcomePersisted;
            record.generation = entry.sequence;
        }
    }
    Ok(operations)
}

fn require_unfinished(record: &OperationLifecycleRecord) -> Result<(), OperationError> {
    if record.outcome().is_some() {
        Err(OperationError::InvalidTransition(
            "lifecycle transition after outcome",
        ))
    } else {
        Ok(())
    }
}
