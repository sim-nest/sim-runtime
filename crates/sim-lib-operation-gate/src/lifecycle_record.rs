//! Verified projected state for one durable operation.

use sim_kernel::Datum;

use crate::{
    OperationAttempt, OperationError, OperationGrant, OperationId, OperationIntent,
    lifecycle::{
        FencedDispatch, LifecycleReceipt, OUTCOME_TAG, OperationLease, OperationObservation,
        OperationOutcome, OperationOutcomeId, OperationStep,
    },
    lifecycle_wire::{outcome_datum, outcome_from_datum},
    operation_wire::{content_id, field, id_from_datum, node_fields},
};

/// Durable identified outcome stored by the lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IdentifiedOutcome {
    pub(super) id: OperationOutcomeId,
    pub(super) operation: OperationId,
    pub(super) outcome: OperationOutcome,
}

impl IdentifiedOutcome {
    pub(super) fn new(
        operation: OperationId,
        outcome: OperationOutcome,
    ) -> Result<Self, OperationError> {
        let datum = outcome_datum(&operation, &outcome);
        Ok(Self {
            id: OperationOutcomeId(content_id(&datum)?),
            operation,
            outcome,
        })
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, OUTCOME_TAG, 3)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let outcome = outcome_from_datum(field(fields, "value")?)?;
        let value = Self::new(operation, outcome)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation outcome"));
        }
        Ok(value)
    }
    pub(super) fn canonical_datum(&self) -> Datum {
        outcome_datum(&self.operation, &self.outcome)
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
    pub(super) receipts: Vec<LifecycleReceipt>,
    pub(super) observations: Vec<OperationObservation>,
    pub(super) outcomes: Vec<IdentifiedOutcome>,
    pub(super) last_step: OperationStep,
}

impl OperationLifecycleRecord {
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
    /// Returns the latest durable outcome.
    pub fn outcome(&self) -> Option<&OperationOutcome> {
        self.outcomes.last().map(|value| &value.outcome)
    }
    /// Returns the semantic identity of the latest durable outcome.
    pub fn outcome_id(&self) -> Option<&OperationOutcomeId> {
        self.outcomes.last().map(|value| &value.id)
    }
    pub(super) fn last_step(&self) -> OperationStep {
        self.last_step
    }
}
