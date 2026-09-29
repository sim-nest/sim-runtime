//! Intent-first resource destinations in the existing operation journal.

use crate::operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields};
use crate::{FencedDispatchId, OperationError};
use sim_kernel::{ContentId, Datum};

/// Exact resource-owner destination persisted before any reservation effect.
///
/// This record is not resource custody, allocation success, preparation, or
/// permission to start payload. The independently surviving resource owner uses
/// its exact destination to rediscover incomplete acquisition after caller loss.
/// It must never replace an uncertain destination with a fresh allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleReservation {
    id: ContentId,
    dispatch: FencedDispatchId,
    destination: Datum,
}

impl LifecycleReservation {
    pub(super) fn new(
        dispatch: FencedDispatchId,
        destination: Datum,
    ) -> Result<Self, OperationError> {
        if destination == Datum::Nil {
            return Err(OperationError::NonCanonical(
                "empty reservation destination",
            ));
        }
        let mut value = Self {
            id: dispatch.content_id().clone(),
            dispatch,
            destination,
        };
        value.id = content_id(&value.canonical_datum())?;
        Ok(value)
    }
    /// Returns this exact destination declaration's semantic identity.
    pub const fn id(&self) -> &ContentId {
        &self.id
    }
    /// Returns the durable dispatch whose resource acquisition is named.
    pub const fn dispatch(&self) -> &FencedDispatchId {
        &self.dispatch
    }
    /// Returns correlation data interpreted only by the selected resource owner.
    pub const fn destination(&self) -> &Datum {
        &self.destination
    }
    /// Returns the canonical intent; its bytes do not confer native authority.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "lifecycle-reservation-intent-v1",
            vec![
                ("dispatch", id_datum(self.dispatch.content_id())),
                ("destination", self.destination.clone()),
            ],
        )
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, "lifecycle-reservation-intent-v1", 2)?;
        let value = Self::new(
            FencedDispatchId(id_from_datum(field(fields, "dispatch")?)?),
            field(fields, "destination")?.clone(),
        )?;
        if !crate::operation_wire::same_datum(&value.canonical_datum(), datum) {
            return Err(OperationError::NonCanonical("reservation intent"));
        }
        Ok(value)
    }
}
