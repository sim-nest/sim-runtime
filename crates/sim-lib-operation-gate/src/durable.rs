//! Durable operation intent, dispatch, and raw performer receipts.

use std::fmt;

use crate::{operation_error::OperationError, operation_wire::*};
use sim_kernel::{CapabilityName, ContentId, Datum, Symbol};

pub(crate) const INTENT_TAG: &str = "intent-v1";
pub(crate) const GRANT_TAG: &str = "grant-v1";
pub(crate) const ATTEMPT_TAG: &str = "attempt-v1";
pub(crate) const DISPATCH_TAG: &str = "dispatch-v1";
pub(crate) const RECEIPT_TAG: &str = "performer-receipt-v1";

/// Replay rule bound into immutable operation intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayPolicy {
    /// A later reconciliation phase may prove that repeat performance is safe.
    Idempotent,
    /// A durable dispatch is an at-most-once barrier, even without an acknowledgement.
    ExactlyOnce,
}

impl ReplayPolicy {
    pub(crate) fn datum(self) -> Datum {
        Datum::Symbol(Symbol::qualified(
            "operation",
            match self {
                Self::Idempotent => "idempotent",
                Self::ExactlyOnce => "exactly-once",
            },
        ))
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        match datum {
            Datum::Symbol(value) if *value == Symbol::qualified("operation", "idempotent") => {
                Ok(Self::Idempotent)
            }
            Datum::Symbol(value) if *value == Symbol::qualified("operation", "exactly-once") => {
                Ok(Self::ExactlyOnce)
            }
            _ => Err(OperationError::NonCanonical("replay policy")),
        }
    }
}

/// Stable identity derived only from canonical immutable operation intent.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationId(pub(crate) ContentId);

/// Identity of canonical operation intent.
pub type OperationIntentId = OperationId;

impl OperationId {
    /// Borrows the kernel semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        render_id(&self.0, formatter)
    }
}

/// Canonical semantic intent whose identity survives grants, attempts, and leases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationIntent {
    pub(crate) id: OperationId,
    pub(crate) operation: String,
    pub(crate) target: Datum,
    pub(crate) intended_result: Datum,
    pub(crate) replay_policy: ReplayPolicy,
}

impl OperationIntent {
    /// Constructs and identifies one immutable intent.
    pub fn new(
        operation: impl Into<String>,
        target: Datum,
        intended_result: Datum,
        replay_policy: ReplayPolicy,
    ) -> Result<Self, OperationError> {
        let operation = operation.into();
        if operation.is_empty() {
            return Err(OperationError::EmptyOperation);
        }
        let datum = intent_datum(&operation, &target, &intended_result, replay_policy);
        Ok(Self {
            id: OperationId(content_id(&datum)?),
            operation,
            target,
            intended_result,
            replay_policy,
        })
    }

    /// Returns the stable operation identity.
    pub const fn id(&self) -> &OperationId {
        &self.id
    }

    /// Returns the open domain operation name.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Returns the exact semantic target.
    pub const fn target(&self) -> &Datum {
        &self.target
    }

    /// Returns the intended semantic postcondition.
    pub const fn intended_result(&self) -> &Datum {
        &self.intended_result
    }

    /// Returns the replay rule bound into this intent.
    pub const fn replay_policy(&self) -> ReplayPolicy {
        self.replay_policy
    }

    /// Returns the exact canonical value whose id is [`Self::id`].
    pub fn canonical_datum(&self) -> Datum {
        intent_datum(
            &self.operation,
            &self.target,
            &self.intended_result,
            self.replay_policy,
        )
    }

    pub(crate) fn verify(&self) -> Result<(), OperationError> {
        if content_id(&self.canonical_datum())? != self.id.0 {
            return Err(OperationError::ContradictoryIntent);
        }
        Ok(())
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, INTENT_TAG, 4)?;
        let operation = string_field(fields, "operation")?.to_owned();
        let target = field(fields, "target")?.clone();
        let intended_result = field(fields, "intended-result")?.clone();
        let replay_policy = ReplayPolicy::from_datum(field(fields, "replay-policy")?)?;
        let value = Self::new(operation, target, intended_result, replay_policy)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation intent"));
        }
        Ok(value)
    }
}

/// Identity of one separately recorded least-authority grant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationGrantId(pub(crate) ContentId);

impl OperationGrantId {
    /// Borrows the grant's semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Exact authority presented for one operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationGrant {
    pub(crate) id: OperationGrantId,
    pub(crate) operation: OperationId,
    pub(crate) capability: CapabilityName,
    pub(crate) authority: Datum,
}

impl OperationGrant {
    /// Constructs a grant independently of any writer lease or attempt.
    pub fn new(
        operation: OperationId,
        capability: CapabilityName,
        authority: Datum,
    ) -> Result<Self, OperationError> {
        let datum = grant_datum(&operation, &capability, &authority);
        Ok(Self {
            id: OperationGrantId(content_id(&datum)?),
            operation,
            capability,
            authority,
        })
    }

    /// Returns the grant identity.
    pub const fn id(&self) -> &OperationGrantId {
        &self.id
    }

    /// Returns the operation this grant can authorize.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// Returns the exact capability recorded by the grant.
    pub const fn capability(&self) -> &CapabilityName {
        &self.capability
    }

    /// Returns the canonical authority evidence supplied by the caller.
    pub const fn authority(&self) -> &Datum {
        &self.authority
    }

    /// Returns the grant's canonical semantic value.
    pub fn canonical_datum(&self) -> Datum {
        grant_datum(&self.operation, &self.capability, &self.authority)
    }

    pub(crate) fn verify(&self) -> Result<(), OperationError> {
        if content_id(&self.canonical_datum())? != self.id.0 {
            return Err(OperationError::NonCanonical("operation grant"));
        }
        Ok(())
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, GRANT_TAG, 3)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let capability = CapabilityName::new(string_field(fields, "capability")?);
        let authority = field(fields, "authority")?.clone();
        let value = Self::new(operation, capability, authority)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation grant"));
        }
        Ok(value)
    }
}

/// Identity of one attempt record, kept separate from the operation id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationAttemptId(pub(crate) ContentId);

/// One caller-selected attempt ordinal for a stable operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationAttempt {
    pub(crate) id: OperationAttemptId,
    pub(crate) operation: OperationId,
    pub(crate) ordinal: u64,
}

impl OperationAttempt {
    /// Constructs an attempt whose identity contains no writer lease.
    pub fn new(operation: OperationId, ordinal: u64) -> Result<Self, OperationError> {
        let datum = attempt_datum(&operation, ordinal);
        Ok(Self {
            id: OperationAttemptId(content_id(&datum)?),
            operation,
            ordinal,
        })
    }

    /// Returns the attempt identity.
    pub const fn id(&self) -> &OperationAttemptId {
        &self.id
    }

    /// Returns the stable operation attempted.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// Returns the caller-selected attempt ordinal.
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Returns the attempt's canonical semantic value.
    pub fn canonical_datum(&self) -> Datum {
        attempt_datum(&self.operation, self.ordinal)
    }

    pub(crate) fn verify(&self) -> Result<(), OperationError> {
        if content_id(&self.canonical_datum())? != self.id.0 {
            return Err(OperationError::NonCanonical("operation attempt"));
        }
        Ok(())
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, ATTEMPT_TAG, 2)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let ordinal = u64_field(fields, "ordinal")?;
        let value = Self::new(operation, ordinal)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation attempt"));
        }
        Ok(value)
    }
}

impl OperationAttemptId {
    /// Borrows the attempt's semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Identity of a dispatch durably recorded before a performer is called.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DispatchId(pub(crate) ContentId);

impl DispatchId {
    /// Borrows the dispatch's semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Exact durable handoff to an injected performer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationDispatch {
    pub(crate) id: DispatchId,
    pub(crate) operation: OperationId,
    pub(crate) grant: OperationGrantId,
    pub(crate) attempt: OperationAttemptId,
}

impl OperationDispatch {
    pub(crate) fn new(
        operation: OperationId,
        grant: OperationGrantId,
        attempt: OperationAttemptId,
    ) -> Result<Self, OperationError> {
        let datum = dispatch_datum(&operation, &grant, &attempt);
        Ok(Self {
            id: DispatchId(content_id(&datum)?),
            operation,
            grant,
            attempt,
        })
    }

    /// Returns the durable dispatch identity.
    pub const fn id(&self) -> &DispatchId {
        &self.id
    }

    /// Returns the stable operation identity.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// Returns the exact separately persisted grant identity.
    pub const fn grant(&self) -> &OperationGrantId {
        &self.grant
    }

    /// Returns the exact separately persisted attempt identity.
    pub const fn attempt(&self) -> &OperationAttemptId {
        &self.attempt
    }

    /// Returns the dispatch's canonical semantic value.
    pub fn canonical_datum(&self) -> Datum {
        dispatch_datum(&self.operation, &self.grant, &self.attempt)
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, DISPATCH_TAG, 3)?;
        let operation = OperationId(id_from_datum(field(fields, "operation")?)?);
        let grant = OperationGrantId(id_from_datum(field(fields, "grant")?)?);
        let attempt = OperationAttemptId(id_from_datum(field(fields, "attempt")?)?);
        let value = Self::new(operation, grant, attempt)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("operation dispatch"));
        }
        Ok(value)
    }
}

/// Identity of a raw performer acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PerformerReceiptId(pub(crate) ContentId);

impl PerformerReceiptId {
    /// Borrows the receipt's semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

/// Raw performer acknowledgement bound to one durable dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerformerReceipt {
    pub(crate) id: PerformerReceiptId,
    pub(crate) dispatch: DispatchId,
    pub(crate) raw: Datum,
}

impl PerformerReceipt {
    pub(crate) fn new(dispatch: DispatchId, raw: Datum) -> Result<Self, OperationError> {
        let datum = receipt_datum(&dispatch, &raw);
        Ok(Self {
            id: PerformerReceiptId(content_id(&datum)?),
            dispatch,
            raw,
        })
    }

    /// Returns the raw receipt identity.
    pub const fn id(&self) -> &PerformerReceiptId {
        &self.id
    }

    /// Returns the dispatch acknowledged by the performer.
    pub const fn dispatch(&self) -> &DispatchId {
        &self.dispatch
    }

    /// Returns the uninterpreted canonical performer response.
    pub const fn raw(&self) -> &Datum {
        &self.raw
    }

    /// Returns the receipt's canonical semantic value.
    pub fn canonical_datum(&self) -> Datum {
        receipt_datum(&self.dispatch, &self.raw)
    }

    pub(crate) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, RECEIPT_TAG, 2)?;
        let dispatch = DispatchId(id_from_datum(field(fields, "dispatch")?)?);
        let raw = field(fields, "raw")?.clone();
        let value = Self::new(dispatch, raw)?;
        if value.canonical_datum() != *datum {
            return Err(OperationError::NonCanonical("performer receipt"));
        }
        Ok(value)
    }
}

/// The three durable states delivered by the operation-log phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DurableOperationState {
    /// Canonical intent and its separate grant are durable.
    IntentPersisted {
        /// Exact semantic intent identity.
        intent: OperationIntentId,
    },
    /// Dispatch is durable; recovery must not call the performer again.
    Dispatched {
        /// Exact semantic intent identity.
        intent: OperationIntentId,
        /// Exact dispatch identity.
        dispatch: DispatchId,
    },
    /// The raw performer acknowledgement is durable.
    ReceiptPersisted {
        /// Exact dispatch identity.
        dispatch: DispatchId,
        /// Exact raw receipt identity.
        receipt: PerformerReceiptId,
    },
}

/// Result of one injected performer call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerformerResponse {
    /// The performer returned a canonical raw acknowledgement.
    Receipt(Datum),
    /// Performance may have occurred, but no acknowledgement arrived.
    AcknowledgementMissing,
}

/// Effect boundary used only after a matching dispatch is durable.
pub trait OperationPerformer {
    /// Performs one dispatch and returns an uninterpreted response.
    fn perform(&mut self, dispatch: &OperationDispatch) -> PerformerResponse;
}
