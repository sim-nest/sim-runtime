//! Canonical independent postcondition observations.

use crate::lifecycle_wire::{with_observation_clock, with_preparation, with_release};
use crate::{
    OperationError, OperationId,
    lifecycle::*,
    lifecycle_wire::{
        observation_base_datum, observation_datum, optional_id, optional_id_datum, response_datum,
        response_from_datum, u64_datum,
    },
    operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields, u64_field},
};
use sim_kernel::{Datum, Symbol};

/// Durable independent observation of one operation postcondition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationObservation {
    pub(super) id: OperationObservationId,
    pub(super) operation: OperationId,
    pub(super) observer: Datum,
    pub(super) response: PostconditionResponse,
    pub(super) dispatch: Option<FencedDispatchId>,
    pub(super) receipt: Option<LifecycleReceiptId>,
    pub(super) preparation: Option<LifecyclePreparationId>,
    pub(super) release: Option<sim_kernel::ContentId>,
    pub(super) clock: Option<Datum>,
    reservation: Option<sim_kernel::ContentId>,
    pub(super) last_durable_step: OperationStep,
    pub(super) observed_at: u64,
    pub(super) evidence: EvidenceSetId,
}

impl OperationObservation {
    pub(super) fn new(
        request: &PostconditionRequest,
        observer: Datum,
        response: PostconditionResponse,
    ) -> Result<Self, OperationError> {
        let base = observation_base_datum(request, &observer, &response);
        let evidence = EvidenceSetId(content_id(&node(
            "evidence-set-v1",
            vec![("observation", base.clone())],
        ))?);
        let datum = observation_datum(request, &observer, &response, &evidence);
        Ok(Self {
            id: OperationObservationId(content_id(&datum)?),
            operation: request.operation.clone(),
            observer,
            response,
            dispatch: request.dispatch.clone(),
            receipt: request.receipt.clone(),
            preparation: request.preparation.as_ref().map(|value| value.id().clone()),
            release: request.release.as_ref().map(|value| value.id().clone()),
            clock: request.clock.clone(),
            reservation: request.reservation.as_ref().map(|value| value.id().clone()),
            last_durable_step: request.last_durable_step,
            observed_at: request.observed_at,
            evidence,
        })
    }
    /// Returns the semantic observation identity.
    pub const fn id(&self) -> &OperationObservationId {
        &self.id
    }
    /// Returns the stable operation observed.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }
    /// Returns the independent observer identity.
    pub const fn observer(&self) -> &Datum {
        &self.observer
    }
    /// Returns the typed observation response.
    pub const fn response(&self) -> &PostconditionResponse {
        &self.response
    }
    /// Returns the dispatch observed, if any.
    pub const fn dispatch(&self) -> Option<&FencedDispatchId> {
        self.dispatch.as_ref()
    }
    /// Returns the latest raw receipt visible to the observer, if any.
    pub const fn receipt(&self) -> Option<&LifecycleReceiptId> {
        self.receipt.as_ref()
    }
    /// Returns the exact pre-execution binding included in this evidence.
    pub const fn preparation(&self) -> Option<&LifecyclePreparationId> {
        self.preparation.as_ref()
    }
    /// Returns the exact release intent included in this evidence, not proof of start.
    pub const fn release(&self) -> Option<&sim_kernel::ContentId> {
        self.release.as_ref()
    }
    /// Returns the exact clock epoch and units bound into the observation evidence.
    pub const fn clock(&self) -> Option<&Datum> {
        self.clock.as_ref()
    }
    /// Returns the exact intent-first resource destination included in evidence.
    pub const fn reservation(&self) -> Option<&sim_kernel::ContentId> {
        self.reservation.as_ref()
    }
    /// Returns the last durable step visible to the observer.
    pub const fn last_durable_step(&self) -> OperationStep {
        self.last_durable_step
    }
    /// Returns the explicit monotonic observation tick.
    pub const fn observed_at(&self) -> u64 {
        self.observed_at
    }
    /// Returns the evidence-set identity derived from the observation.
    pub const fn evidence(&self) -> &EvidenceSetId {
        &self.evidence
    }
    /// Returns the canonical semantic observation value.
    pub fn canonical_datum(&self) -> Datum {
        self.stored_datum()
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let (reservation, unreserved) = if matches!(datum, Datum::Node { tag, .. }
            if tag == &Symbol::qualified("operation", "reserved-observation-v1"))
        {
            let fields = node_fields(datum, "reserved-observation-v1", 2)?;
            (
                Some(id_from_datum(field(fields, "reservation")?)?),
                field(fields, "observation")?,
            )
        } else {
            (None, datum)
        };
        let (clock, inner) = if matches!(unreserved, Datum::Node { tag, .. }
            if tag == &Symbol::qualified("operation", "clocked-observation-v1"))
        {
            let fields = node_fields(unreserved, "clocked-observation-v1", 2)?;
            let clock = field(fields, "clock")?.clone();
            if clock == Datum::Nil {
                return Err(OperationError::InvalidLease);
            }
            (Some(clock), field(fields, "observation")?)
        } else {
            (None, unreserved)
        };
        let released = matches!(inner, Datum::Node { tag, .. }
            if tag == &Symbol::qualified("operation", "postcondition-release-observation-v1"));
        let prepared = released
            || matches!(inner, Datum::Node { tag, .. }
            if tag == &Symbol::qualified("operation", "postcondition-prepared-observation-v1"));
        let fields = node_fields(
            inner,
            if released {
                "postcondition-release-observation-v1"
            } else if prepared {
                "postcondition-prepared-observation-v1"
            } else {
                OBSERVATION_TAG
            },
            if released {
                10
            } else if prepared {
                9
            } else {
                8
            },
        )?;
        let release = if released {
            Some(id_from_datum(field(fields, "release")?)?)
        } else {
            None
        };
        let preparation = if prepared {
            Some(LifecyclePreparationId(id_from_datum(field(
                fields,
                "preparation",
            )?)?))
        } else {
            None
        };
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let observer = field(fields, "observer")?.clone();
        let response = response_from_datum(field(fields, "response")?)?;
        let dispatch = optional_id(field(fields, "dispatch")?)?.map(FencedDispatchId);
        let last_durable_step = OperationStep::from_datum(field(fields, "last-durable-step")?)?;
        let observed_at = u64_field(fields, "observed-at")?;
        let receipt = optional_id(field(fields, "receipt")?)?.map(LifecycleReceiptId);
        let evidence = EvidenceSetId(id_from_datum(field(fields, "evidence")?)?);
        let evidence_input = with_observation_clock(
            with_release(
                with_preparation(
                    node(
                        "observation-evidence-v1",
                        vec![
                            ("operation", id_datum(operation.content_id())),
                            ("observer", observer.clone()),
                            ("response", response_datum(&response)),
                            (
                                "dispatch",
                                optional_id_datum(
                                    dispatch.as_ref().map(FencedDispatchId::content_id),
                                ),
                            ),
                            (
                                "receipt",
                                optional_id_datum(
                                    receipt.as_ref().map(LifecycleReceiptId::content_id),
                                ),
                            ),
                            ("last-durable-step", last_durable_step.canonical_datum()),
                            ("observed-at", u64_datum(observed_at)),
                        ],
                    ),
                    preparation.as_ref(),
                    "observation-prepared-evidence-v1",
                ),
                release.as_ref(),
                "observation-release-evidence-v1",
            ),
            clock.as_ref(),
            reservation.as_ref(),
        );
        let expected_evidence = EvidenceSetId(content_id(&node(
            "evidence-set-v1",
            vec![("observation", evidence_input)],
        ))?);
        if evidence != expected_evidence {
            return Err(OperationError::NonCanonical("observation evidence set"));
        }
        let id = OperationObservationId(content_id(datum)?);
        let value = Self {
            id,
            operation,
            observer,
            response,
            dispatch,
            receipt,
            preparation,
            release,
            clock,
            reservation,
            last_durable_step,
            observed_at,
            evidence,
        };
        if !crate::operation_wire::same_datum(&value.stored_datum(), datum) {
            return Err(OperationError::NonCanonical("operation observation"));
        }
        Ok(value)
    }
    pub(super) fn stored_datum(&self) -> Datum {
        with_observation_clock(
            with_release(
                with_preparation(
                    node(
                        OBSERVATION_TAG,
                        vec![
                            ("operation", id_datum(self.operation.content_id())),
                            ("observer", self.observer.clone()),
                            ("response", response_datum(&self.response)),
                            (
                                "dispatch",
                                optional_id_datum(
                                    self.dispatch.as_ref().map(FencedDispatchId::content_id),
                                ),
                            ),
                            (
                                "receipt",
                                optional_id_datum(
                                    self.receipt.as_ref().map(LifecycleReceiptId::content_id),
                                ),
                            ),
                            (
                                "last-durable-step",
                                self.last_durable_step.canonical_datum(),
                            ),
                            ("observed-at", u64_datum(self.observed_at)),
                            ("evidence", id_datum(self.evidence.content_id())),
                        ],
                    ),
                    self.preparation.as_ref(),
                    "postcondition-prepared-observation-v1",
                ),
                self.release.as_ref(),
                "postcondition-release-observation-v1",
            ),
            self.clock.as_ref(),
            self.reservation.as_ref(),
        )
    }
}
