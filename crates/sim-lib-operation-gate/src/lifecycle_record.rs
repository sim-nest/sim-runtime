//! Verified projected state for one durable operation.

use sim_kernel::{ContentId, Datum};
use sim_lib_journal::JournalHead;

use crate::{
    LifecyclePreparation, OperationAttempt, OperationError, OperationGrant, OperationId,
    OperationIntent,
    lifecycle::{
        FencedDispatch, LifecycleReceipt, OUTCOME_TAG, OperationLease, OperationObservation,
        OperationObservationId, OperationOutcome, OperationOutcomeId, OperationStep,
    },
    lifecycle_wire::{outcome_datum, outcome_from_datum},
    operation_wire::{content_id, field, id_from_datum, node_fields},
};

/// Durable identified outcome stored by the lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IdentifiedOutcome {
    pub(super) id: OperationOutcomeId,
    pub(super) operation: OperationId,
    pub(super) observation: OperationObservationId,
    pub(super) outcome: OperationOutcome,
}

impl IdentifiedOutcome {
    pub(super) fn new(
        operation: OperationId,
        observation: OperationObservationId,
        outcome: OperationOutcome,
    ) -> Result<Self, OperationError> {
        let datum = outcome_datum(&operation, &observation, &outcome);
        Ok(Self {
            id: OperationOutcomeId(content_id(&datum)?),
            operation,
            observation,
            outcome,
        })
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, OUTCOME_TAG, 4)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let observation = OperationObservationId(id_from_datum(field(fields, "observation")?)?);
        let outcome = outcome_from_datum(field(fields, "value")?)?;
        let value = Self::new(operation, observation, outcome)?;
        if !crate::operation_wire::same_datum(&value.canonical_datum(), datum) {
            return Err(OperationError::NonCanonical("operation outcome"));
        }
        Ok(value)
    }
    pub(super) fn canonical_datum(&self) -> Datum {
        outcome_datum(&self.operation, &self.observation, &self.outcome)
    }
}

/// Complete verified lifecycle history reconstructed only from the journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationLifecycleRecord {
    pub(super) intent: OperationIntent,
    pub(super) grant: OperationGrant,
    pub(super) leases: Vec<OperationLease>,
    pub(super) attempts: Vec<OperationAttempt>,
    pub(super) dispatches: Vec<FencedDispatch>,
    pub(super) preparations: Vec<LifecyclePreparation>,
    pub(super) reservations: Vec<crate::LifecycleReservation>,
    pub(super) releases: Vec<crate::LifecycleRelease>,
    pub(super) cancellation: Option<crate::LifecycleCancellation>,
    pub(super) receipts: Vec<LifecycleReceipt>,
    pub(super) observations: Vec<OperationObservation>,
    pub(super) outcome: Option<IdentifiedOutcome>,
    pub(super) last_step: OperationStep,
    /// The journal sequence number of the most recent entry that touched
    /// this operation (any kind), including the intent itself. Monotonic
    /// per record. Not the journal's global head sequence -- this
    /// operation's own most recent entry only.
    pub(super) generation: u64,
    /// `generation`'s value at the moment the record's current
    /// `observations.last()` was itself recorded. Equal to `generation`
    /// exactly when that observation is still the single most recent fact
    /// about this record; less than it the instant anything else (a new
    /// lease, dispatch, reservation, preparation, release, or receipt) is
    /// recorded afterward. This is the one durable, monotonic basis for
    /// "is this observation still current" -- checked by equality, not by
    /// comparing individual derived fields (dispatch identity, tick
    /// values) against each other one at a time.
    pub(super) observation_generation: Option<u64>,
}

/// One operation projection bound to the head of the exact verified journal read.
///
/// The optional record and head are deliberately inseparable: callers cannot
/// splice a semantic projection from one read onto a later journal generation.
/// This value is observation only. It carries no writer, lease, replay, or
/// execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedOperationRecord {
    pub(super) journal_head: Option<JournalHead>,
    pub(super) record: Option<OperationLifecycleRecord>,
}

/// Owned proof that one exact verified operation read agrees with the retained
/// performer and independent observer contracts.
///
/// Construction is intentionally private. Historical records remain available
/// for diagnosis. This value proves contract consistency, not provider
/// authority, checker qualification, or revocation currentness.
///
/// ```compile_fail
/// use sim_kernel::Datum;
/// use sim_lib_operation_gate::VerifiedOperationRecord;
/// # fn forge(read: &VerifiedOperationRecord, performer: &Datum, observer: &Datum) {
/// let _ = read.validate_retained_contract(performer, observer);
/// # }
/// ```
#[derive(Debug)]
pub struct ContractVerifiedOperation {
    verified: VerifiedOperationRecord,
}

impl VerifiedOperationRecord {
    pub(super) const fn new(
        journal_head: Option<JournalHead>,
        record: Option<OperationLifecycleRecord>,
    ) -> Self {
        Self {
            journal_head,
            record,
        }
    }

    /// Returns the exact verified journal-head entry identity, when non-empty.
    pub fn journal_head(&self) -> Option<&ContentId> {
        self.journal_head.as_ref().map(|head| &head.entry)
    }

    /// Returns the exact verified journal-head sequence, when non-empty.
    pub fn journal_sequence(&self) -> Option<u64> {
        self.journal_head.as_ref().map(|head| head.sequence)
    }

    /// Returns the operation record projected from that same verified read.
    pub const fn record(&self) -> Option<&OperationLifecycleRecord> {
        self.record.as_ref()
    }

    /// Consumes the observation and returns its operation record.
    pub fn into_record(self) -> Option<OperationLifecycleRecord> {
        self.record
    }

    /// Validates retained contracts against this exact read and returns an
    /// opaque contract-consistent view.
    ///
    /// This performs no journal read and grants no execution or replay authority.
    /// The caller supplies the contracts retained by its custody value. This
    /// comparison does not establish platform or checker authority.
    pub(crate) fn validate_retained_contract(
        &self,
        performer: &Datum,
        observer: &Datum,
    ) -> Result<ContractVerifiedOperation, OperationError> {
        validate_contract_identities(performer, observer)?;
        if let Some(record) = &self.record {
            record.validate_retained_contract(performer, observer)?;
        }
        Ok(ContractVerifiedOperation {
            verified: self.clone(),
        })
    }
}

impl ContractVerifiedOperation {
    /// Returns the journal-head entry from the exact contract-consistent read.
    pub fn journal_head(&self) -> Option<&ContentId> {
        self.verified.journal_head()
    }

    /// Returns the journal sequence from the exact contract-consistent read.
    pub fn journal_sequence(&self) -> Option<u64> {
        self.verified.journal_sequence()
    }

    /// Returns the operation record from the exact contract-consistent read.
    pub const fn record(&self) -> Option<&OperationLifecycleRecord> {
        self.verified.record()
    }

    /// Consumes the contract proof and returns its exact verified read.
    ///
    /// Neither value carries platform or checker qualification authority.
    pub fn into_verified(self) -> VerifiedOperationRecord {
        self.verified
    }
}

impl OperationLifecycleRecord {
    /// Verifies that a historical positive outcome still agrees with the
    /// exact performer and observer contracts retained for this read.
    ///
    /// This performs no journal read and grants no execution authority. It is
    /// intended to be applied to the same [`VerifiedOperationRecord`] that a
    /// read-only receipt projection consumes.
    pub(super) fn validate_retained_contract(
        &self,
        performer: &Datum,
        observer: &Datum,
    ) -> Result<(), OperationError> {
        if self
            .dispatches
            .iter()
            .any(|dispatch| !crate::operation_wire::same_datum(dispatch.performer(), performer))
        {
            return Err(OperationError::PerformerMismatch);
        }
        if matches!(
            self.outcome(),
            Some(OperationOutcome::AlreadyTrue { .. } | OperationOutcome::Verified { .. })
        ) {
            let Some(bound) = self.outcome.as_ref().and_then(|outcome| {
                self.observations
                    .iter()
                    .find(|observation| observation.id() == &outcome.observation)
            }) else {
                return Err(OperationError::ObserverMismatch);
            };
            if !crate::operation_wire::same_datum(bound.observer(), observer) {
                return Err(OperationError::ObserverMismatch);
            }
        }
        Ok(())
    }

    /// Returns the sticky stop intent, never proof that resources are quiescent.
    /// It does not replace the latest execution/observation progress step.
    pub const fn cancellation(&self) -> Option<&crate::LifecycleCancellation> {
        self.cancellation.as_ref()
    }
    /// Returns intent-first resource destinations, not allocation receipts.
    pub fn reservations(&self) -> &[crate::LifecycleReservation] {
        &self.reservations
    }
    /// Returns durable release intents, which do not establish payload start.
    pub fn releases(&self) -> &[crate::LifecycleRelease] {
        &self.releases
    }
    /// Returns pre-execution resource bindings in durable order.
    pub fn preparations(&self) -> &[LifecyclePreparation] {
        &self.preparations
    }
    /// Returns the immutable intent.
    pub const fn intent(&self) -> &OperationIntent {
        &self.intent
    }
    /// Returns the separately recorded grant.
    pub const fn grant(&self) -> &OperationGrant {
        &self.grant
    }
    /// Returns every bounded lease in durable order.
    pub fn leases(&self) -> &[OperationLease] {
        &self.leases
    }
    /// Returns every attempt in durable order.
    pub fn attempts(&self) -> &[OperationAttempt] {
        &self.attempts
    }
    /// Returns every dispatch in durable order.
    pub fn dispatches(&self) -> &[FencedDispatch] {
        &self.dispatches
    }
    /// Returns every raw receipt in durable order.
    pub fn receipts(&self) -> &[LifecycleReceipt] {
        &self.receipts
    }
    /// Returns every independent observation in durable order.
    pub fn observations(&self) -> &[OperationObservation] {
        &self.observations
    }
    /// Returns the immutable terminal outcome.
    pub fn outcome(&self) -> Option<&OperationOutcome> {
        self.outcome.as_ref().map(|value| &value.outcome)
    }
    /// Returns the semantic identity of the immutable terminal outcome.
    pub fn outcome_id(&self) -> Option<&OperationOutcomeId> {
        self.outcome.as_ref().map(|value| &value.id)
    }
    /// Returns the exact independent observation consumed by the terminal outcome.
    pub fn outcome_observation(&self) -> Option<&OperationObservationId> {
        self.outcome.as_ref().map(|value| &value.observation)
    }
    /// Returns the latest durable lifecycle boundary in this verified record.
    pub const fn last_step(&self) -> OperationStep {
        self.last_step
    }
}

fn validate_contract_identities(performer: &Datum, observer: &Datum) -> Result<(), OperationError> {
    crate::operation_wire::content_id(performer)?;
    crate::operation_wire::content_id(observer)?;
    if crate::operation_wire::same_datum(performer, observer) {
        return Err(OperationError::ObserverNotIndependent);
    }
    Ok(())
}
