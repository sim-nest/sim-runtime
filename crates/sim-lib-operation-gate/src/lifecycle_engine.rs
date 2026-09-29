//! Journal-backed operation execution and recovery coordinator.

use std::sync::Arc;

use sim_kernel::{Datum, Symbol};
use sim_lib_journal::{
    Journal, JournalBackend, JournalEntry, JournalObject, Lease, VerifiedSnapshot,
};

use crate::{
    LifecyclePreparation, OperationAttempt, OperationError, OperationGrant, OperationId,
    OperationIntent, ReplayPolicy,
    lifecycle::{
        FencedDispatch, LeaseWindow, LifecyclePerformer, LifecyclePerformerResponse,
        LifecycleReceipt, OperationLease, OperationObservation, OperationOutcome, OperationStep,
        PostconditionObserver, PostconditionRequest, PostconditionResponse,
    },
    lifecycle_project::{project, project_verified_lifecycle_records},
    lifecycle_record::{IdentifiedOutcome, OperationLifecycleRecord, VerifiedOperationRecord},
};

/// Journal-backed M5 coordinator for fenced dispatch and independent reconciliation.
pub struct OperationLifecycle<B: JournalBackend> {
    pub(super) journal: Arc<Journal<Arc<B>>>,
    pub(super) lease: Option<Lease>,
    pub(super) clock: Option<Arc<dyn crate::LeaseClock>>,
}

impl<B: JournalBackend> OperationLifecycle<B> {
    /// Creates a lifecycle over one backend value.
    pub fn new(backend: B) -> Self {
        Self::from_shared(Arc::new(backend))
    }
    /// Creates a lifecycle over a shared backend for crash/reopen recovery.
    pub fn from_shared(backend: Arc<B>) -> Self {
        Self {
            journal: Arc::new(Journal::new(backend)),
            lease: None,
            clock: None,
        }
    }
    /// Selects the clock owner for scoped leases and prepared release admission.
    pub fn with_clock(mut self, clock: Arc<dyn crate::LeaseClock>) -> Self {
        self.clock = Some(clock);
        self
    }
    /// Reconstructs one complete lifecycle solely from the verified journal.
    pub fn record(
        &self,
        operation: &OperationId,
    ) -> Result<Option<OperationLifecycleRecord>, OperationError> {
        Ok(self.verified_record(operation)?.into_record())
    }

    /// Reads one verified snapshot and binds its head to this operation's projection.
    ///
    /// The result carries no journal writer or replay authority. Its head and
    /// optional record are guaranteed to derive from the same backend read.
    pub fn verified_record(
        &self,
        operation: &OperationId,
    ) -> Result<VerifiedOperationRecord, OperationError> {
        Self::verified_record_from_snapshot(&self.journal.verified_snapshot()?, operation)
    }

    /// Binds one projection to the head of the supplied exact verified snapshot.
    ///
    /// This method performs no backend read and acquires no writer.
    pub fn verified_record_from_snapshot(
        snapshot: &VerifiedSnapshot,
        operation: &OperationId,
    ) -> Result<VerifiedOperationRecord, OperationError> {
        Ok(VerifiedOperationRecord::new(
            snapshot.head().cloned(),
            Self::record_from_verified_snapshot(snapshot, operation)?,
        ))
    }

    /// Reconstructs one complete lifecycle solely from the supplied verified snapshot.
    ///
    /// This projection performs no backend read and acquires no writer. Callers
    /// that independently bind a physical state envelope to a semantic snapshot
    /// can therefore derive the record from that exact same read generation.
    pub fn record_from_verified_snapshot(
        snapshot: &VerifiedSnapshot,
        operation: &OperationId,
    ) -> Result<Option<OperationLifecycleRecord>, OperationError> {
        Ok(project_verified_lifecycle_records(snapshot)?.remove(operation))
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
        self.accept(intent, grant, window)
            .map_err(crate::OperationAcceptanceError::into_error)?
            .run(performer, observer)
    }

    pub(super) fn run_accepted(
        &mut self,
        intent: &OperationIntent,
        grant: &OperationGrant,
        window: LeaseWindow,
        performer: &mut dyn LifecyclePerformer,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationOutcome, OperationError> {
        self.validate_clock(&window)?;
        let performer_identity = performer.identity();
        let observer_identity = observer.identity();
        crate::operation_wire::content_id(&performer_identity)?;
        crate::operation_wire::content_id(&observer_identity)?;
        if crate::operation_wire::same_datum(&performer_identity, &observer_identity) {
            return Err(OperationError::ObserverNotIndependent);
        }
        let records = project(self.journal.verified_snapshot()?)?;
        if let Some(existing) = records.get(intent.id()) {
            if existing.intent.id() != intent.id() {
                return Err(OperationError::ContradictoryIntent);
            }
            if existing.grant.id() != grant.id() {
                return Err(OperationError::GrantMismatch);
            }
            if existing.dispatches.iter().any(|dispatch| {
                !crate::operation_wire::same_datum(dispatch.performer(), &performer_identity)
            }) {
                return Err(OperationError::PerformerMismatch);
            }
            if let Some(outcome) = existing.outcome() {
                if matches!(
                    outcome,
                    OperationOutcome::AlreadyTrue { .. } | OperationOutcome::Verified { .. }
                ) {
                    existing.validate_retained_contract(&performer_identity, &observer_identity)?;
                }
                return Ok(outcome.clone());
            }
            if existing.leases.last().is_some_and(|lease| {
                !crate::operation_wire::same_optional_datum(lease.clock(), window.clock())
                    || window.acquired_at() < lease.acquired_at()
            }) {
                return Err(OperationError::InvalidLease);
            }
        } else {
            return Err(OperationError::NotResumed);
        }

        let record = records
            .get(intent.id())
            .expect("intent was persisted")
            .clone();
        let observation = self.observe(&record, &performer_identity, &window, observer)?;
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
                return self.persist_outcome(intent.id(), observation.id(), outcome);
            }
            PostconditionResponse::Unavailable { .. } | PostconditionResponse::Disputed { .. } => {
                return self.persist_outcome(
                    intent.id(),
                    observation.id(),
                    OperationOutcome::Uncertain {
                        last_durable_step: OperationStep::ObservationPersisted,
                    },
                );
            }
            PostconditionResponse::NotSatisfied { .. } if record.cancellation().is_some() => {
                return self.persist_outcome(
                    intent.id(),
                    observation.id(),
                    OperationOutcome::Uncertain {
                        last_durable_step: OperationStep::ObservationPersisted,
                    },
                );
            }
            PostconditionResponse::NotSatisfied { observed, .. }
                if !record.dispatches.is_empty() =>
            {
                // A negative postcondition is not a resource-disposition proof.
                // Reserved attempts require custody-backed reconciliation, even
                // for idempotent effects and expired execution leases.
                if !record.reservations.is_empty() || !record.preparations.is_empty() {
                    return self.persist_outcome(
                        intent.id(),
                        observation.id(),
                        OperationOutcome::Uncertain {
                            last_durable_step: OperationStep::ObservationPersisted,
                        },
                    );
                }
                // Proceeding requires positive proof the lease's window
                // fully elapsed (same clock domain, observed_at >=
                // expires_at) -- a live lease means the original performer
                // may still complete, and an observation that merely fails
                // is_live_at numerically (an unrelated clock, or one that
                // precedes acquisition) proves nothing either way: sealing
                // Diverged now would be premature even for ExactlyOnce,
                // which has no retry lease to fall back on the way
                // Idempotent does -- it must wait, not seal a verdict.
                if !record.leases.last().is_some_and(|lease| {
                    lease.expired_as_of(observation.clock(), observation.observed_at())
                }) {
                    return self.persist_outcome(
                        intent.id(),
                        observation.id(),
                        OperationOutcome::Uncertain {
                            last_durable_step: OperationStep::ObservationPersisted,
                        },
                    );
                }
                if intent.replay_policy() == ReplayPolicy::ExactlyOnce {
                    return self.persist_outcome(
                        intent.id(),
                        observation.id(),
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
                    return Err(OperationError::InvalidLease);
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
        )?
        .in_clock(window.clock.clone())?;
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

        let reservation = if let Some(destination) = performer.plan_reservation(&dispatch)? {
            let reservation = crate::LifecycleReservation::new(dispatch.id().clone(), destination)?;
            self.append(
                "lifecycle-reservation-intent-persisted",
                vec![reservation.canonical_datum()],
            )?;
            Some(reservation)
        } else {
            None
        };
        let binding = match &reservation {
            Some(reservation) => Some(performer.prepare_reserved(&dispatch, reservation)?),
            None => performer.prepare(&dispatch)?,
        };
        let is_direct_perform = binding.is_none();
        let response = if let Some(binding) = binding {
            let acknowledged = (|| {
                let preparation = LifecyclePreparation::new(dispatch.id().clone(), binding)?
                    .in_reservation(reservation.as_ref())?;
                self.append(
                    "lifecycle-preparation-persisted",
                    vec![preparation.canonical_datum()],
                )?;
                Ok::<_, OperationError>(preparation)
            })();
            let preparation = match acknowledged {
                Ok(preparation) => preparation,
                Err(cause) => {
                    return Err(
                        match performer.preparation_acknowledgement_failed(&dispatch, &cause) {
                            Ok(()) => cause,
                            Err(disposition) => OperationError::PreparationDisposition {
                                cause: Box::new(cause),
                                disposition: Box::new(disposition),
                            },
                        },
                    );
                }
            };
            let mut admission = crate::lifecycle_release::PreparedAdmission {
                journal: &self.journal,
                writer: self.lease.as_ref().expect("writer acquired"),
                clock: self.clock.as_deref(),
                lease: &operation_lease,
                dispatch: &dispatch,
                preparation: &preparation,
                attempted: false,
            };
            performer.perform_prepared(&dispatch, &preparation, &mut admission)?
        } else {
            performer.perform(&dispatch)
        };
        if let LifecyclePerformerResponse::Receipt(raw) = response {
            let record = self
                .record(intent.id())?
                .expect("lifecycle remains present");
            let release = record
                .releases()
                .iter()
                .find(|value| value.dispatch() == dispatch.id())
                .map(|value| value.id().clone());
            let receipt = LifecycleReceipt::new(dispatch.id().clone(), raw, release)?;
            // The prepared path's own release admission already re-checked
            // lease liveness atomically at commit time; the direct
            // (unprepared) path never did, so it needs the same guard here,
            // right before this operation's effect is durably recorded as
            // having actually completed.
            if is_direct_perform {
                self.append_gated(
                    "lifecycle-receipt-persisted",
                    vec![receipt.canonical_datum()],
                    &operation_lease,
                )?;
            } else {
                self.append(
                    "lifecycle-receipt-persisted",
                    vec![receipt.canonical_datum()],
                )?;
            }
        }
        let updated = self
            .record(intent.id())?
            .expect("lifecycle remains present");
        let observation = self.observe(&updated, &performer_identity, &window, observer)?;
        // Re-read after observation, not the `updated` snapshot from before
        // it: cancellation (durable, via the observer or a concurrent
        // caller) or a late reservation/preparation admission can land while
        // `observe` runs, and this decision must see that, not a stale read.
        let current = self
            .record(intent.id())?
            .expect("lifecycle remains present");
        let outcome = match observation.response() {
            PostconditionResponse::Satisfied { .. } => OperationOutcome::Verified {
                evidence: observation.evidence.clone(),
            },
            // A negative postcondition is not a resource-disposition proof
            // (same rule the recovery path above already applies): custody
            // this dispatch created (a reservation, a preparation) or a
            // durable cancellation may not have reached final resolution,
            // so treating it as a terminal Diverged here would strand it
            // rather than let later custody-backed reconciliation resolve
            // it.
            PostconditionResponse::NotSatisfied { .. }
                if current.cancellation().is_some()
                    || !current.reservations().is_empty()
                    || !current.preparations().is_empty() =>
            {
                OperationOutcome::Uncertain {
                    last_durable_step: OperationStep::ObservationPersisted,
                }
            }
            // Sealing Diverged requires positive proof the lease's window
            // fully elapsed (same clock domain, observed_at >= expires_at):
            // a live lease means the original performer may still
            // complete, for either replay policy, and an observation that
            // merely fails is_live_at numerically (an unrelated clock, or
            // one that precedes acquisition) proves nothing either way.
            // This mirrors the identical check the recovery path above
            // applies; here it guards the far more common case, a negative
            // observation taken immediately after a fresh dispatch while
            // that dispatch's own lease is still running.
            PostconditionResponse::NotSatisfied { .. }
                if !current.leases.last().is_some_and(|lease| {
                    lease.expired_as_of(observation.clock(), observation.observed_at())
                }) =>
            {
                OperationOutcome::Uncertain {
                    last_durable_step: OperationStep::ObservationPersisted,
                }
            }
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
        self.persist_outcome(intent.id(), observation.id(), outcome)
    }

    fn observe(
        &mut self,
        record: &OperationLifecycleRecord,
        performer: &Datum,
        window: &LeaseWindow,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationObservation, OperationError> {
        let (clock, observed_at) = match &self.clock {
            Some(clock) => {
                let reading = clock.read()?;
                if !crate::operation_wire::same_optional_datum(
                    Some(&reading.domain),
                    window.clock(),
                ) || reading.tick < window.acquired_at()
                {
                    return Err(OperationError::InvalidLease);
                }
                (Some(reading.domain), reading.tick)
            }
            None => (None, window.acquired_at()),
        };
        let request = PostconditionRequest {
            reservation: record
                .dispatches
                .last()
                .and_then(|dispatch| {
                    record
                        .reservations
                        .iter()
                        .find(|value| value.dispatch() == dispatch.id())
                })
                .cloned(),
            clock,
            release: record
                .dispatches
                .last()
                .and_then(|dispatch| {
                    record
                        .releases
                        .iter()
                        .find(|value| value.dispatch() == dispatch.id())
                })
                .cloned(),
            preparation: record
                .dispatches
                .last()
                .and_then(|dispatch| {
                    record
                        .preparations
                        .iter()
                        .find(|value| value.dispatch() == dispatch.id())
                })
                .cloned(),
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
        crate::operation_wire::content_id(&observer_identity)?;
        if crate::operation_wire::same_datum(&observer_identity, performer) {
            return Err(OperationError::ObserverNotIndependent);
        }
        let response = observer.observe(&request);
        let response = if (request.reservation.is_some() || request.preparation.is_some())
            && request.release.is_none()
            && matches!(response, PostconditionResponse::Satisfied { .. })
        {
            PostconditionResponse::Disputed {
                first: crate::lifecycle_wire::response_datum(&response),
                second: Datum::String(
                    "reserved or prepared success lacks durable release intent".into(),
                ),
            }
        } else {
            response
        };
        let observation = OperationObservation::new(&request, observer_identity, response)?;
        self.append(
            "lifecycle-observation-persisted",
            vec![observation.stored_datum()],
        )?;
        Ok(observation)
    }

    fn persist_outcome(
        &mut self,
        operation: &OperationId,
        observation: &crate::OperationObservationId,
        outcome: OperationOutcome,
    ) -> Result<OperationOutcome, OperationError> {
        let identified =
            IdentifiedOutcome::new(operation.clone(), observation.clone(), outcome.clone())?;
        self.append(
            "lifecycle-outcome-persisted",
            vec![identified.canonical_datum()],
        )?;
        Ok(outcome)
    }

    pub(super) fn append(
        &self,
        kind: &'static str,
        datums: Vec<Datum>,
    ) -> Result<(), OperationError> {
        let lease = self.lease.as_ref().ok_or(OperationError::NotResumed)?;
        let objects = datums
            .into_iter()
            .map(JournalObject::from_datum)
            .collect::<Result<Vec<_>, _>>()?;
        let payloads = objects.iter().map(|object| object.id.clone()).collect();
        let snapshot = self.journal.verified_snapshot()?;
        let expected = snapshot.head().cloned();
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
        crate::lifecycle_project::validate_extension(&snapshot, &entry, &objects)?;
        self.journal
            .publish(lease, expected.as_ref(), objects, vec![entry])?;
        Ok(())
    }

    /// Same durable write as [`Self::append`], but only for the direct
    /// (unprepared) perform path: the write still commits either way (the
    /// journal is append-only and this is not resource-disposition proof
    /// either), but the operation lease's liveness is re-checked atomically
    /// at actual commit time, not merely at accept time, matching the same
    /// [`crate::lifecycle_release::PreparedAdmission::admit`] rule the
    /// guarded prepared-release path already applies. `operation_lease` is
    /// this call's own semantic lease, distinct from `self.lease` (the
    /// journal-fencing writer lease used for the write itself). When no
    /// engine clock is configured this degrades to a plain [`Self::append`],
    /// the same no-clock convention [`Self::observe`] already uses -- there
    /// is nothing to gate against, not a refusal condition.
    pub(super) fn append_gated(
        &self,
        kind: &'static str,
        datums: Vec<Datum>,
        operation_lease: &OperationLease,
    ) -> Result<(), OperationError> {
        let Some(clock) = self.clock.as_deref() else {
            return self.append(kind, datums);
        };
        let writer = self.lease.as_ref().ok_or(OperationError::NotResumed)?;
        let objects = datums
            .into_iter()
            .map(JournalObject::from_datum)
            .collect::<Result<Vec<_>, _>>()?;
        let payloads = objects.iter().map(|object| object.id.clone()).collect();
        let snapshot = self.journal.verified_snapshot()?;
        let expected = snapshot.head().cloned();
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
        crate::lifecycle_project::validate_extension(&snapshot, &entry, &objects)?;
        let mut refusal = None;
        let mut gate = || match clock.read() {
            Ok(reading)
                if crate::operation_wire::same_optional_datum(
                    Some(&reading.domain),
                    operation_lease.clock(),
                ) && operation_lease.is_live_at(reading.tick) => {}
            Ok(_) => refusal = Some(OperationError::InvalidLease),
            Err(error) => refusal = Some(error),
        };
        self.journal
            .publish_then(writer, expected.as_ref(), objects, vec![entry], &mut gate)?;
        match refusal {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
