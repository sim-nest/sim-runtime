//! Reconciled M5 operation lifecycle over the canonical journal.

use std::fmt;

use sim_kernel::{ContentId, Datum, Symbol};

use crate::{
    OperationError, OperationId,
    lifecycle_wire::{
        fenced_dispatch_datum, lease_datum, lifecycle_receipt_datum, observation_base_datum,
        observation_datum, optional_id, optional_id_datum, response_datum, response_from_datum,
        u64_datum,
    },
    operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields, u64_field},
};

pub(super) const LEASE_TAG: &str = "bounded-lease-v1";
pub(super) const DISPATCH_TAG: &str = "fenced-dispatch-v1";
pub(super) const RECEIPT_TAG: &str = "lifecycle-receipt-v1";
pub(super) const OBSERVATION_TAG: &str = "postcondition-observation-v1";
pub(super) const OUTCOME_TAG: &str = "outcome-v1";

macro_rules! semantic_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(pub(super) ContentId);

        impl $name {
            /// Borrows the semantic content identity.
            pub const fn content_id(&self) -> &ContentId {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                crate::operation_wire::render_id(&self.0, formatter)
            }
        }
    };
}

semantic_id!(
    OperationLeaseId,
    "Identity of one bounded, fenced operation lease."
);
semantic_id!(
    FencedDispatchId,
    "Identity of one lease-bound durable dispatch."
);
semantic_id!(
    LifecycleReceiptId,
    "Identity of one raw lifecycle performer receipt."
);
semantic_id!(
    OperationObservationId,
    "Identity of one independent postcondition observation."
);
semantic_id!(
    EvidenceSetId,
    "Identity of the evidence carried by one observation."
);
semantic_id!(
    OperationOutcomeId,
    "Identity of one durable reconciliation outcome."
);

/// Explicit holder and monotonic bounds for one operation lease acquisition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseWindow {
    pub(super) holder: Datum,
    pub(super) acquired_at: u64,
    pub(super) expires_at: u64,
}

impl LeaseWindow {
    /// Validates a non-empty half-open monotonic lease interval.
    pub fn new(holder: Datum, acquired_at: u64, expires_at: u64) -> Result<Self, OperationError> {
        if expires_at <= acquired_at {
            return Err(OperationError::InvalidLease);
        }
        Ok(Self {
            holder,
            acquired_at,
            expires_at,
        })
    }

    /// Returns the explicit holder identity.
    pub const fn holder(&self) -> &Datum {
        &self.holder
    }

    /// Returns the inclusive monotonic acquisition tick.
    pub const fn acquired_at(&self) -> u64 {
        self.acquired_at
    }

    /// Returns the exclusive monotonic expiry tick.
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Bounded effect authority tied to the journal writer fence that recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationLease {
    pub(super) id: OperationLeaseId,
    pub(super) operation: OperationId,
    pub(super) holder: Datum,
    pub(super) fence: u64,
    pub(super) acquired_at: u64,
    pub(super) expires_at: u64,
}

impl OperationLease {
    pub(super) fn new(
        operation: OperationId,
        holder: Datum,
        fence: u64,
        acquired_at: u64,
        expires_at: u64,
    ) -> Result<Self, OperationError> {
        if expires_at <= acquired_at {
            return Err(OperationError::InvalidLease);
        }
        let datum = lease_datum(&operation, &holder, fence, acquired_at, expires_at);
        Ok(Self {
            id: OperationLeaseId(content_id(&datum)?),
            operation,
            holder,
            fence,
            acquired_at,
            expires_at,
        })
    }

    /// Returns the semantic lease identity.
    pub const fn id(&self) -> &OperationLeaseId {
        &self.id
    }
    /// Returns the stable operation protected by the lease.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }
    /// Returns the explicit holder identity.
    pub const fn holder(&self) -> &Datum {
        &self.holder
    }
    /// Returns the journal writer fence bound into the lease.
    pub const fn fence(&self) -> u64 {
        self.fence
    }
    /// Returns the caller-supplied monotonic acquisition tick.
    pub const fn acquired_at(&self) -> u64 {
        self.acquired_at
    }
    /// Returns the exclusive caller-supplied monotonic expiry tick.
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
    /// Returns true while the explicit monotonic time remains inside the bound.
    pub const fn is_live_at(&self, now: u64) -> bool {
        self.acquired_at <= now && now < self.expires_at
    }
    /// Returns the canonical semantic lease value.
    pub fn canonical_datum(&self) -> Datum {
        lease_datum(
            &self.operation,
            &self.holder,
            self.fence,
            self.acquired_at,
            self.expires_at,
        )
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, LEASE_TAG, 5)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let holder = field(fields, "holder")?.clone();
        let value = Self::new(
            operation,
            holder,
            u64_field(fields, "fence")?,
            u64_field(fields, "acquired-at")?,
            u64_field(fields, "expires-at")?,
        )?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation lease"));
        }
        Ok(value)
    }
}

/// Durable performer handoff bound to an exact grant, attempt, and fenced lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FencedDispatch {
    pub(super) id: FencedDispatchId,
    pub(super) operation: OperationId,
    pub(super) grant: crate::OperationGrantId,
    pub(super) attempt: crate::OperationAttemptId,
    pub(super) lease: OperationLeaseId,
    pub(super) performer: Datum,
}

impl FencedDispatch {
    pub(super) fn new(
        operation: OperationId,
        grant: crate::OperationGrantId,
        attempt: crate::OperationAttemptId,
        lease: OperationLeaseId,
        performer: Datum,
    ) -> Result<Self, OperationError> {
        let datum = fenced_dispatch_datum(&operation, &grant, &attempt, &lease, &performer);
        Ok(Self {
            id: FencedDispatchId(content_id(&datum)?),
            operation,
            grant,
            attempt,
            lease,
            performer,
        })
    }
    /// Returns the dispatch identity supplied as the performer idempotency token.
    pub const fn id(&self) -> &FencedDispatchId {
        &self.id
    }
    /// Returns the stable semantic operation identity.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }
    /// Returns the separately persisted least-authority grant identity.
    pub const fn grant(&self) -> &crate::OperationGrantId {
        &self.grant
    }
    /// Returns the separately persisted attempt identity.
    pub const fn attempt(&self) -> &crate::OperationAttemptId {
        &self.attempt
    }
    /// Returns the bounded lease authorizing this dispatch.
    pub const fn lease(&self) -> &OperationLeaseId {
        &self.lease
    }
    /// Returns the exact performer authority selected for this handoff.
    pub const fn performer(&self) -> &Datum {
        &self.performer
    }
    /// Returns the canonical semantic dispatch value.
    pub fn canonical_datum(&self) -> Datum {
        fenced_dispatch_datum(
            &self.operation,
            &self.grant,
            &self.attempt,
            &self.lease,
            &self.performer,
        )
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, DISPATCH_TAG, 5)?;
        let value = Self::new(
            OperationId(id_from_datum(field(fields, "operation")?)?),
            crate::OperationGrantId(id_from_datum(field(fields, "grant")?)?),
            crate::OperationAttemptId(id_from_datum(field(fields, "attempt")?)?),
            OperationLeaseId(id_from_datum(field(fields, "lease")?)?),
            field(fields, "performer")?.clone(),
        )?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("fenced dispatch"));
        }
        Ok(value)
    }
}

/// Raw performer acknowledgement bound to one fenced dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleReceipt {
    pub(super) id: LifecycleReceiptId,
    pub(super) dispatch: FencedDispatchId,
    pub(super) raw: Datum,
}

impl LifecycleReceipt {
    pub(super) fn new(dispatch: FencedDispatchId, raw: Datum) -> Result<Self, OperationError> {
        let datum = lifecycle_receipt_datum(&dispatch, &raw);
        Ok(Self {
            id: LifecycleReceiptId(content_id(&datum)?),
            dispatch,
            raw,
        })
    }
    /// Returns the semantic receipt identity.
    pub const fn id(&self) -> &LifecycleReceiptId {
        &self.id
    }
    /// Returns the acknowledged dispatch.
    pub const fn dispatch(&self) -> &FencedDispatchId {
        &self.dispatch
    }
    /// Returns the uninterpreted raw acknowledgement.
    pub const fn raw(&self) -> &Datum {
        &self.raw
    }
    /// Returns the canonical receipt value.
    pub fn canonical_datum(&self) -> Datum {
        lifecycle_receipt_datum(&self.dispatch, &self.raw)
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, RECEIPT_TAG, 2)?;
        let value = Self::new(
            FencedDispatchId(id_from_datum(field(fields, "dispatch")?)?),
            field(fields, "raw")?.clone(),
        )?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("lifecycle receipt"));
        }
        Ok(value)
    }
}

/// Last durable lifecycle boundary available to reconciliation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationStep {
    /// Intent and grant are durable.
    IntentPersisted,
    /// A bounded, fenced operation lease is durable.
    LeaseAcquired,
    /// The performer handoff is durable.
    DispatchPersisted,
    /// The raw performer acknowledgement is durable.
    ReceiptPersisted,
    /// Independent postcondition evidence is durable.
    ObservationPersisted,
    /// A reconciliation outcome is durable.
    OutcomePersisted,
}

impl OperationStep {
    pub(super) fn datum(self) -> Datum {
        Datum::Symbol(Symbol::qualified(
            "operation-step",
            match self {
                Self::IntentPersisted => "intent-persisted",
                Self::LeaseAcquired => "lease-acquired",
                Self::DispatchPersisted => "dispatch-persisted",
                Self::ReceiptPersisted => "receipt-persisted",
                Self::ObservationPersisted => "observation-persisted",
                Self::OutcomePersisted => "outcome-persisted",
            },
        ))
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let Datum::Symbol(value) = datum else {
            return Err(OperationError::NonCanonical("operation step"));
        };
        for step in [
            Self::IntentPersisted,
            Self::LeaseAcquired,
            Self::DispatchPersisted,
            Self::ReceiptPersisted,
            Self::ObservationPersisted,
            Self::OutcomePersisted,
        ] {
            if step.datum() == Datum::Symbol(value.clone()) {
                return Ok(step);
            }
        }
        Err(OperationError::NonCanonical("operation step"))
    }
}

/// Request passed to an independent observer without performer authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostconditionRequest {
    pub(super) operation: OperationId,
    pub(super) target: Datum,
    pub(super) expected: Datum,
    pub(super) dispatch: Option<FencedDispatchId>,
    pub(super) receipt: Option<LifecycleReceiptId>,
    pub(super) last_durable_step: OperationStep,
    pub(super) observed_at: u64,
}

impl PostconditionRequest {
    /// Returns the semantic operation identity.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }
    /// Returns the exact target to inspect.
    pub const fn target(&self) -> &Datum {
        &self.target
    }
    /// Returns the expected semantic postcondition.
    pub const fn expected(&self) -> &Datum {
        &self.expected
    }
    /// Returns the latest dispatch, when performance was durably authorized.
    pub const fn dispatch(&self) -> Option<&FencedDispatchId> {
        self.dispatch.as_ref()
    }
    /// Returns the latest raw receipt identity, when one became durable.
    pub const fn receipt(&self) -> Option<&LifecycleReceiptId> {
        self.receipt.as_ref()
    }
    /// Returns the last durable lifecycle boundary known before observation.
    pub const fn last_durable_step(&self) -> OperationStep {
        self.last_durable_step
    }
    /// Returns the caller-supplied monotonic tick at which observation began.
    pub const fn observed_at(&self) -> u64 {
        self.observed_at
    }
}

/// Typed result from a postcondition observer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostconditionResponse {
    /// The intended postcondition is present.
    Satisfied {
        /// Exact semantic value observed.
        observed: Datum,
        /// Observer-owned evidence describing the read.
        evidence: Datum,
    },
    /// The observer proved that the intended postcondition is absent.
    NotSatisfied {
        /// Exact semantic value observed instead.
        observed: Datum,
        /// Observer-owned evidence describing the read.
        evidence: Datum,
    },
    /// The observer could not establish a value.
    Unavailable {
        /// Typed reason no trustworthy value could be obtained.
        reason: Datum,
    },
    /// Independent observation sources disagreed.
    Disputed {
        /// First incompatible observation.
        first: Datum,
        /// Second incompatible observation.
        second: Datum,
    },
}

/// Effect-free identity plus independently performed postcondition observation.
pub trait PostconditionObserver {
    /// Returns the stable observer authority identity.
    fn identity(&self) -> Datum;
    /// Observes the postcondition without performing the requested operation.
    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse;
}

/// Result of calling a lifecycle performer after durable dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecyclePerformerResponse {
    /// The performer returned an uninterpreted raw acknowledgement.
    Receipt(Datum),
    /// Performance may have occurred, but its acknowledgement was lost.
    AcknowledgementMissing,
}

/// Effect authority invoked only after a matching fenced dispatch is durable.
pub trait LifecyclePerformer {
    /// Returns the stable performer authority identity.
    fn identity(&self) -> Datum;
    /// Performs the exact durable dispatch once.
    fn perform(&mut self, dispatch: &FencedDispatch) -> LifecyclePerformerResponse;
}

/// Durable independent observation of one operation postcondition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationObservation {
    pub(super) id: OperationObservationId,
    pub(super) operation: OperationId,
    pub(super) observer: Datum,
    pub(super) response: PostconditionResponse,
    pub(super) dispatch: Option<FencedDispatchId>,
    pub(super) receipt: Option<LifecycleReceiptId>,
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
        let fields = node_fields(datum, OBSERVATION_TAG, 8)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let observer = field(fields, "observer")?.clone();
        let response = response_from_datum(field(fields, "response")?)?;
        let dispatch = optional_id(field(fields, "dispatch")?)?.map(FencedDispatchId);
        let last_durable_step = OperationStep::from_datum(field(fields, "last-durable-step")?)?;
        let observed_at = u64_field(fields, "observed-at")?;
        let receipt = optional_id(field(fields, "receipt")?)?.map(LifecycleReceiptId);
        let evidence = EvidenceSetId(id_from_datum(field(fields, "evidence")?)?);
        let evidence_input = node(
            "observation-evidence-v1",
            vec![
                ("operation", id_datum(operation.content_id())),
                ("observer", observer.clone()),
                ("response", response_datum(&response)),
                (
                    "dispatch",
                    optional_id_datum(dispatch.as_ref().map(FencedDispatchId::content_id)),
                ),
                (
                    "receipt",
                    optional_id_datum(receipt.as_ref().map(LifecycleReceiptId::content_id)),
                ),
                ("last-durable-step", last_durable_step.datum()),
                ("observed-at", u64_datum(observed_at)),
            ],
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
            last_durable_step,
            observed_at,
            evidence,
        };
        if value.stored_datum() != *datum {
            return Err(OperationError::NonCanonical("operation observation"));
        }
        Ok(value)
    }
    pub(super) fn stored_datum(&self) -> Datum {
        node(
            OBSERVATION_TAG,
            vec![
                ("operation", id_datum(self.operation.content_id())),
                ("observer", self.observer.clone()),
                ("response", response_datum(&self.response)),
                (
                    "dispatch",
                    optional_id_datum(self.dispatch.as_ref().map(FencedDispatchId::content_id)),
                ),
                (
                    "receipt",
                    optional_id_datum(self.receipt.as_ref().map(LifecycleReceiptId::content_id)),
                ),
                ("last-durable-step", self.last_durable_step.datum()),
                ("observed-at", u64_datum(self.observed_at)),
                ("evidence", id_datum(self.evidence.content_id())),
            ],
        )
    }
}

/// Reconciled semantic operation result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    /// Observation proved the intended state before any dispatch.
    AlreadyTrue {
        /// Independent evidence proving the pre-existing postcondition.
        evidence: EvidenceSetId,
    },
    /// Observation proved the intended state after a durable dispatch.
    Verified {
        /// Independent evidence proving the post-dispatch postcondition.
        evidence: EvidenceSetId,
    },
    /// Observation proved a different state.
    Diverged {
        /// Exact independently observed value.
        observed: Datum,
        /// Exact intended value.
        expected: Datum,
    },
    /// Available evidence cannot establish completion or safe replay.
    Uncertain {
        /// Last lifecycle fact known durably.
        last_durable_step: OperationStep,
    },
}
