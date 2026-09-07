//! Journal-backed operation execution and recovery coordinator.

use std::sync::Arc;

use sim_kernel::{Datum, Symbol};
use sim_lib_journal::{Journal, JournalBackend, JournalEntry, JournalObject, Lease};

use crate::{
    OperationAttempt, OperationError, OperationGrant, OperationId, OperationIntent, ReplayPolicy,
    lifecycle::{
        FencedDispatch, LeaseWindow, LifecyclePerformer, LifecyclePerformerResponse,
        LifecycleReceipt, OperationLease, OperationObservation, OperationOutcome, OperationStep,
        PostconditionObserver, PostconditionRequest, PostconditionResponse,
    },
    lifecycle_project::project,
    lifecycle_record::{IdentifiedOutcome, OperationLifecycleRecord},
};

/// Journal-backed M5 coordinator for fenced dispatch and independent reconciliation.
pub struct OperationLifecycle<B: JournalBackend> {
    journal: Journal<Arc<B>>,
    lease: Option<Lease>,
}

impl<B: JournalBackend> OperationLifecycle<B> {
    /// Creates a lifecycle over one backend value.
    pub fn new(backend: B) -> Self {
        Self::from_shared(Arc::new(backend))
    }
    /// Creates a lifecycle over a shared backend for crash/reopen recovery.
    pub fn from_shared(backend: Arc<B>) -> Self {
        Self {
            journal: Journal::new(backend),
            lease: None,
        }
    }
    /// Reconstructs one complete lifecycle solely from the verified journal.
    pub fn record(
        &self,
        operation: &OperationId,
    ) -> Result<Option<OperationLifecycleRecord>, OperationError> {
        Ok(project(self.journal.verified_snapshot()?)?.remove(operation))
    }

    /// Advances one operation, observing before any new or repeated performance.
    pub fn run(
        &mut self,
        intent: &OperationIntent,
        grant: &OperationGrant,
        window: LeaseWindow,
        performer: &mut dyn LifecyclePerformer,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationOutcome, OperationError> {
        intent.verify()?;
        grant.verify()?;
        if grant.operation != intent.id {
            return Err(OperationError::GrantMismatch);
        }
        let performer_identity = performer.identity();
        let observer_identity = observer.identity();
        if performer_identity == observer_identity {
            return Err(OperationError::ObserverNotIndependent);
        }
        self.lease = Some(self.journal.acquire_lease()?);

        let mut records = project(self.journal.verified_snapshot()?)?;
        if let Some(existing) = records.get(intent.id()) {
            if existing.intent != *intent {
                return Err(OperationError::ContradictoryIntent);
            }
            if existing.grant != *grant {
                return Err(OperationError::GrantMismatch);
            }
            if existing
                .dispatches
                .iter()
                .any(|dispatch| dispatch.performer() != &performer_identity)
            {
                return Err(OperationError::PerformerMismatch);
            }
            if let Some(
                outcome
                @ (OperationOutcome::AlreadyTrue { .. } | OperationOutcome::Verified { .. }),
            ) = existing.outcome()
            {
                return Ok(outcome.clone());
            }
            if existing
                .leases
                .last()
                .is_some_and(|lease| window.acquired_at() < lease.acquired_at())
            {
                return Err(OperationError::InvalidLease);
            }
            if intent.replay_policy() == ReplayPolicy::ExactlyOnce
                && matches!(existing.outcome(), Some(OperationOutcome::Diverged { .. }))
            {
                return Ok(existing.outcome().expect("matched outcome").clone());
            }
        } else {
            self.append(
                "lifecycle-intent-persisted",
                vec![intent.canonical_datum(), grant.canonical_datum()],
            )?;
            records = project(self.journal.verified_snapshot()?)?;
        }

        let record = records
            .get(intent.id())
            .expect("intent was persisted")
            .clone();
        let observation =
            self.observe(&record, &performer_identity, window.acquired_at(), observer)?;
        match observation.response() {
            PostconditionResponse::Satisfied { .. } => {
                let outcome = if record.dispatches.is_empty() {
                    OperationOutcome::AlreadyTrue {
                        evidence: observation.evidence.clone(),
                    }
                } else {
                    OperationOutcome::Verified {
                        evidence: observation.evidence.clone(),
                    }
                };
                return self.persist_outcome(intent.id(), outcome);
            }
            PostconditionResponse::Unavailable { .. } | PostconditionResponse::Disputed { .. } => {
                return self.persist_outcome(
                    intent.id(),
                    OperationOutcome::Uncertain {
                        last_durable_step: OperationStep::ObservationPersisted,
                    },
                );
            }
            PostconditionResponse::NotSatisfied { observed, .. }
                if !record.dispatches.is_empty() =>
            {
                if intent.replay_policy() == ReplayPolicy::ExactlyOnce {
                    return self.persist_outcome(
                        intent.id(),
                        OperationOutcome::Diverged {
                            observed: observed.clone(),
                            expected: intent.intended_result().clone(),
                        },
                    );
                }
                if record
                    .leases
                    .last()
                    .is_some_and(|lease| window.acquired_at < lease.expires_at())
                {
                    return self.persist_outcome(
                        intent.id(),
                        OperationOutcome::Uncertain {
                            last_durable_step: OperationStep::ObservationPersisted,
                        },
                    );
                }
            }
            PostconditionResponse::NotSatisfied { .. } => {}
        }

        let writer_fence = self.lease.as_ref().expect("writer lease acquired").fence();
        let operation_lease = OperationLease::new(
            intent.id().clone(),
            window.holder().clone(),
            writer_fence,
            window.acquired_at(),
            window.expires_at(),
        )?;
        self.append(
            "lifecycle-lease-acquired",
            vec![operation_lease.canonical_datum()],
        )?;
        let ordinal =
            u64::try_from(record.attempts.len()).map_err(|_| OperationError::SequenceExhausted)?;
        let attempt = OperationAttempt::new(intent.id().clone(), ordinal)?;
        let dispatch = FencedDispatch::new(
            intent.id().clone(),
            grant.id().clone(),
            attempt.id().clone(),
            operation_lease.id().clone(),
            performer_identity.clone(),
        )?;
        self.append(
            "lifecycle-dispatch-persisted",
            vec![dispatch.canonical_datum(), attempt.canonical_datum()],
        )?;

        if let LifecyclePerformerResponse::Receipt(raw) = performer.perform(&dispatch) {
            let receipt = LifecycleReceipt::new(dispatch.id().clone(), raw)?;
            self.append(
                "lifecycle-receipt-persisted",
                vec![receipt.canonical_datum()],
            )?;
        }
        let updated = self
            .record(intent.id())?
            .expect("lifecycle remains present");
        let observation = self.observe(
            &updated,
            &performer_identity,
            window.acquired_at(),
            observer,
        )?;
        let outcome = match observation.response() {
            PostconditionResponse::Satisfied { .. } => OperationOutcome::Verified {
                evidence: observation.evidence.clone(),
            },
            PostconditionResponse::NotSatisfied { observed, .. } => OperationOutcome::Diverged {
                observed: observed.clone(),
                expected: intent.intended_result().clone(),
            },
            PostconditionResponse::Unavailable { .. } | PostconditionResponse::Disputed { .. } => {
                OperationOutcome::Uncertain {
                    last_durable_step: OperationStep::ObservationPersisted,
                }
            }
        };
        self.persist_outcome(intent.id(), outcome)
    }

    fn observe(
        &mut self,
        record: &OperationLifecycleRecord,
        performer: &Datum,
        observed_at: u64,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationObservation, OperationError> {
        let request = PostconditionRequest {
            operation: record.intent.id().clone(),
            target: record.intent.target().clone(),
            expected: record.intent.intended_result().clone(),
            dispatch: record.dispatches.last().map(|value| value.id.clone()),
            receipt: record
                .dispatches
                .last()
                .and_then(|dispatch| {
                    record
                        .receipts
                        .iter()
                        .rev()
                        .find(|receipt| receipt.dispatch == dispatch.id)
                })
                .map(|value| value.id.clone()),
            last_durable_step: record.last_step(),
            observed_at,
        };
        let observer_identity = observer.identity();
        if &observer_identity == performer {
            return Err(OperationError::ObserverNotIndependent);
        }
        let observation =
            OperationObservation::new(&request, observer_identity, observer.observe(&request))?;
        self.append(
            "lifecycle-observation-persisted",
            vec![observation.stored_datum()],
        )?;
        Ok(observation)
    }

    fn persist_outcome(
        &mut self,
        operation: &OperationId,
        outcome: OperationOutcome,
    ) -> Result<OperationOutcome, OperationError> {
        let identified = IdentifiedOutcome::new(operation.clone(), outcome.clone())?;
        self.append(
            "lifecycle-outcome-persisted",
            vec![identified.canonical_datum()],
        )?;
        Ok(outcome)
    }

    fn append(&self, kind: &'static str, datums: Vec<Datum>) -> Result<(), OperationError> {
        let lease = self.lease.as_ref().ok_or(OperationError::NotResumed)?;
        let objects = datums
            .into_iter()
            .map(JournalObject::from_datum)
            .collect::<Result<Vec<_>, _>>()?;
        let payloads = objects.iter().map(|object| object.id.clone()).collect();
        let expected = self.journal.head()?;
        let sequence = expected
            .as_ref()
            .map_or(Some(0), |head| head.sequence.checked_add(1))
            .ok_or(OperationError::SequenceExhausted)?;
        let entry = JournalEntry::new(
            sequence,
            expected.as_ref().map(|head| head.entry.clone()),
            Symbol::qualified("operation", kind),
            payloads,
        );
        self.journal
            .publish(lease, expected.as_ref(), objects, vec![entry])?;
        Ok(())
    }
}
