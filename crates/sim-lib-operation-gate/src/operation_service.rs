use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::{
    Journal, JournalBackend, JournalEntry, JournalObject, Lease, VerifiedSnapshot,
};

use crate::{durable::*, operation_error::OperationError};

/// Complete verified durable record for one operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationRecord {
    intent: OperationIntent,
    grant: OperationGrant,
    attempt: Option<OperationAttempt>,
    dispatch: Option<OperationDispatch>,
    receipt: Option<PerformerReceipt>,
}

impl OperationRecord {
    /// Returns the immutable semantic intent.
    pub const fn intent(&self) -> &OperationIntent {
        &self.intent
    }

    /// Returns the separately recorded grant.
    pub const fn grant(&self) -> &OperationGrant {
        &self.grant
    }

    /// Returns the attempt selected before dispatch, if dispatch occurred.
    pub const fn attempt(&self) -> Option<&OperationAttempt> {
        self.attempt.as_ref()
    }

    /// Returns the durable performer handoff, if it occurred.
    pub const fn dispatch(&self) -> Option<&OperationDispatch> {
        self.dispatch.as_ref()
    }

    /// Returns the raw performer acknowledgement, if it became durable.
    pub const fn receipt(&self) -> Option<&PerformerReceipt> {
        self.receipt.as_ref()
    }

    /// Derives the exact three-state public projection.
    pub fn state(&self) -> DurableOperationState {
        Projection {
            intent: self.intent.clone(),
            grant: self.grant.clone(),
            dispatch: self.dispatch.clone(),
            attempt: self.attempt.clone(),
            receipt: self.receipt.clone(),
        }
        .state()
    }
}

/// Journal-backed owner of durable operation intent, dispatch, and raw receipts.
pub struct OperationService<B: JournalBackend> {
    journal: Journal<Arc<B>>,
    lease: Option<Lease>,
}

impl<B: JournalBackend> OperationService<B> {
    /// Creates a service over one backend value without acquiring authority.
    pub fn new(backend: B) -> Self {
        Self::from_shared(Arc::new(backend))
    }

    /// Creates a service over a shared backend, supporting crash/reopen tests.
    pub fn from_shared(backend: Arc<B>) -> Self {
        Self {
            journal: Journal::new(backend),
            lease: None,
        }
    }

    /// Acquires a fresh fenced journal lease after start or recovery.
    pub fn resume(&mut self) -> Result<(), OperationError> {
        self.lease = Some(self.journal.acquire_lease()?);
        Ok(())
    }

    /// Reconstructs one operation solely from a verified journal snapshot.
    pub fn state(
        &self,
        operation: &OperationId,
    ) -> Result<Option<DurableOperationState>, OperationError> {
        let projections = project(self.journal.verified_snapshot()?)?;
        Ok(projections.get(operation).map(Projection::state))
    }

    /// Reconstructs the complete durable record from one verified snapshot.
    pub fn record(
        &self,
        operation: &OperationId,
    ) -> Result<Option<OperationRecord>, OperationError> {
        let projections = project(self.journal.verified_snapshot()?)?;
        Ok(projections.get(operation).map(Projection::record))
    }

    /// Persists intent and dispatch, calls the performer once, then persists its receipt.
    ///
    /// If recovery finds a durable dispatch, this method returns that state and
    /// never calls the performer. Missing acknowledgement likewise returns the
    /// durable dispatch state; it is not converted into failure or retry authority.
    pub fn execute(
        &mut self,
        intent: &OperationIntent,
        grant: &OperationGrant,
        attempt: &OperationAttempt,
        performer: &mut dyn OperationPerformer,
    ) -> Result<DurableOperationState, OperationError> {
        self.lease.as_ref().ok_or(OperationError::NotResumed)?;
        intent.verify()?;
        grant.verify()?;
        attempt.verify()?;
        if grant.operation != intent.id {
            return Err(OperationError::GrantMismatch);
        }
        if attempt.operation != intent.id {
            return Err(OperationError::AttemptMismatch);
        }

        let projections = project(self.journal.verified_snapshot()?)?;
        match projections.get(intent.id()) {
            Some(existing) if existing.intent != *intent => {
                return Err(OperationError::ContradictoryIntent);
            }
            Some(existing) if existing.dispatch.is_some() => return Ok(existing.state()),
            Some(existing) if existing.grant != *grant => {
                return Err(OperationError::GrantMismatch);
            }
            Some(_) => {}
            None => self.append(
                "intent-persisted",
                vec![intent.canonical_datum(), grant.canonical_datum()],
            )?,
        }

        let dispatch =
            OperationDispatch::new(intent.id.clone(), grant.id.clone(), attempt.id.clone())?;
        self.append(
            "dispatched",
            vec![dispatch.canonical_datum(), attempt.canonical_datum()],
        )?;
        let raw = match performer.perform(&dispatch) {
            PerformerResponse::Receipt(raw) => raw,
            PerformerResponse::AcknowledgementMissing => {
                return Ok(DurableOperationState::Dispatched {
                    intent: intent.id.clone(),
                    dispatch: dispatch.id,
                });
            }
        };
        let receipt = PerformerReceipt::new(dispatch.id.clone(), raw)?;
        self.append("receipt-persisted", vec![receipt.canonical_datum()])?;
        Ok(DurableOperationState::ReceiptPersisted {
            dispatch: dispatch.id,
            receipt: receipt.id,
        })
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

#[derive(Clone)]
struct Projection {
    intent: OperationIntent,
    grant: OperationGrant,
    dispatch: Option<OperationDispatch>,
    attempt: Option<OperationAttempt>,
    receipt: Option<PerformerReceipt>,
}

impl Projection {
    fn state(&self) -> DurableOperationState {
        match (&self.dispatch, &self.receipt) {
            (Some(dispatch), Some(receipt)) => DurableOperationState::ReceiptPersisted {
                dispatch: dispatch.id.clone(),
                receipt: receipt.id.clone(),
            },
            (Some(dispatch), None) => DurableOperationState::Dispatched {
                intent: self.intent.id.clone(),
                dispatch: dispatch.id.clone(),
            },
            (None, None) => DurableOperationState::IntentPersisted {
                intent: self.intent.id.clone(),
            },
            (None, Some(_)) => unreachable!("projection construction refuses receipt first"),
        }
    }

    fn record(&self) -> OperationRecord {
        OperationRecord {
            intent: self.intent.clone(),
            grant: self.grant.clone(),
            attempt: self.attempt.clone(),
            dispatch: self.dispatch.clone(),
            receipt: self.receipt.clone(),
        }
    }
}

fn project(
    snapshot: VerifiedSnapshot,
) -> Result<BTreeMap<OperationId, Projection>, OperationError> {
    let mut operations = BTreeMap::<OperationId, Projection>::new();
    for entry in snapshot.entries() {
        if entry.kind == Symbol::qualified("operation", "intent-persisted") {
            require_payload_count(entry, 2)?;
            let intent =
                OperationIntent::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let grant = OperationGrant::from_datum(snapshot_datum(&snapshot, &entry.payloads[1])?)?;
            if grant.operation != intent.id {
                return Err(OperationError::GrantMismatch);
            }
            if operations
                .insert(
                    intent.id.clone(),
                    Projection {
                        intent,
                        grant,
                        dispatch: None,
                        attempt: None,
                        receipt: None,
                    },
                )
                .is_some()
            {
                return Err(OperationError::DuplicateIntent);
            }
        } else if entry.kind == Symbol::qualified("operation", "dispatched") {
            require_payload_count(entry, 2)?;
            let dispatch =
                OperationDispatch::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let attempt =
                OperationAttempt::from_datum(snapshot_datum(&snapshot, &entry.payloads[1])?)?;
            let projection = operations
                .get_mut(&dispatch.operation)
                .ok_or(OperationError::InvalidTransition("dispatch before intent"))?;
            if projection.dispatch.is_some()
                || dispatch.grant != projection.grant.id
                || attempt.operation != projection.intent.id
                || dispatch.attempt != attempt.id
            {
                return Err(OperationError::InvalidTransition("conflicting dispatch"));
            }
            projection.dispatch = Some(dispatch);
            projection.attempt = Some(attempt);
        } else if entry.kind == Symbol::qualified("operation", "receipt-persisted") {
            require_payload_count(entry, 1)?;
            let receipt =
                PerformerReceipt::from_datum(snapshot_datum(&snapshot, &entry.payloads[0])?)?;
            let projection = operations
                .values_mut()
                .find(|projection| {
                    projection
                        .dispatch
                        .as_ref()
                        .is_some_and(|dispatch| dispatch.id == receipt.dispatch)
                })
                .ok_or(OperationError::InvalidTransition("receipt before dispatch"))?;
            if projection.receipt.replace(receipt).is_some() {
                return Err(OperationError::InvalidTransition("duplicate receipt"));
            }
        }
    }
    Ok(operations)
}
fn snapshot_datum<'a>(
    snapshot: &'a VerifiedSnapshot,
    id: &ContentId,
) -> Result<&'a Datum, OperationError> {
    snapshot
        .datum(id)
        .ok_or(OperationError::NonCanonical("missing operation payload"))
}

fn require_payload_count(entry: &JournalEntry, expected: usize) -> Result<(), OperationError> {
    if entry.payloads.len() != expected {
        return Err(OperationError::NonCanonical("operation event payloads"));
    }
    Ok(())
}
