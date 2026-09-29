//! Durable binding between fenced dispatch and payload release.

use crate::{
    FencedDispatchId, OperationError,
    lifecycle::LifecyclePreparationId,
    operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields},
};
use sim_kernel::{ContentId, Datum, Symbol};

#[cfg(test)]
mod tests;

/// Resource-owner preparation recorded before the requested payload starts.
///
/// This binds opaque native service/workspace identity to the existing fenced
/// dispatch. It is not an execution result or independent postcondition proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecyclePreparation {
    pub(super) id: LifecyclePreparationId,
    pub(super) dispatch: FencedDispatchId,
    pub(super) binding: Datum,
    reservation: Option<ContentId>,
}

impl LifecyclePreparation {
    pub(super) fn new(dispatch: FencedDispatchId, binding: Datum) -> Result<Self, OperationError> {
        let datum = preparation_datum(&dispatch, &binding);
        Ok(Self {
            id: LifecyclePreparationId(content_id(&datum)?),
            dispatch,
            binding,
            reservation: None,
        })
    }
    pub(super) fn in_reservation(
        mut self,
        reservation: Option<&crate::LifecycleReservation>,
    ) -> Result<Self, OperationError> {
        if let Some(reservation) = reservation {
            if reservation.dispatch() != &self.dispatch {
                return Err(OperationError::InvalidTransition(
                    "preparation reservation dispatch differs",
                ));
            }
            self.reservation = Some(reservation.id().clone());
            self.id = LifecyclePreparationId(content_id(&self.canonical_datum())?);
        }
        Ok(self)
    }
    /// Returns the durable destination whose actual resources were captured.
    /// Legacy preparations have no intent-first destination declaration.
    pub const fn reservation(&self) -> Option<&ContentId> {
        self.reservation.as_ref()
    }
    /// Returns the immutable preparation identity.
    pub const fn id(&self) -> &LifecyclePreparationId {
        &self.id
    }
    /// Returns the exact fenced dispatch authorized to use this binding.
    pub const fn dispatch(&self) -> &FencedDispatchId {
        &self.dispatch
    }
    /// Returns opaque resource-owner evidence, never an execution receipt.
    pub const fn binding(&self) -> &Datum {
        &self.binding
    }
    /// Returns the canonical semantic value retained by the journal.
    pub fn canonical_datum(&self) -> Datum {
        match &self.reservation {
            None => preparation_datum(&self.dispatch, &self.binding),
            Some(reservation) => node(
                "lifecycle-reserved-preparation-v1",
                vec![
                    ("dispatch", id_datum(self.dispatch.content_id())),
                    ("reservation", id_datum(reservation)),
                    ("binding", self.binding.clone()),
                ],
            ),
        }
    }
    /// Decodes the existing canonical preparation DATA, deriving its identity.
    ///
    /// Accepts the legacy unreserved or explicit reserved preparation schema.
    /// Equivalent canonical field ordering has the same identity. This checks
    /// representation only: it does not prove journal publication, reservation
    /// ownership, native custody, current eligibility, or permission to release.
    /// Consumers must join the decoded dispatch, reservation and binding to
    /// their independently retained original owners and verified journal.
    ///
    /// # Errors
    /// Returns [`OperationError::NonCanonical`] for a malformed schema, field,
    /// content identity or noncanonical binding. The input is not modified.
    ///
    /// ```
    /// use sim_kernel::Datum;
    /// use sim_lib_operation_gate::{LifecyclePreparation, OperationError};
    /// assert!(matches!(
    ///     LifecyclePreparation::from_datum(&Datum::Nil),
    ///     Err(OperationError::NonCanonical(_))
    /// ));
    /// ```
    pub fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let reserved = matches!(datum, Datum::Node { tag, .. } if tag == &Symbol::qualified("operation", "lifecycle-reserved-preparation-v1"));
        let fields = node_fields(
            datum,
            if reserved {
                "lifecycle-reserved-preparation-v1"
            } else {
                "lifecycle-preparation-v1"
            },
            if reserved { 3 } else { 2 },
        )?;
        let mut value = Self::new(
            FencedDispatchId(id_from_datum(field(fields, "dispatch")?)?),
            field(fields, "binding")?.clone(),
        )?;
        if reserved {
            value.reservation = Some(id_from_datum(field(fields, "reservation")?)?);
            value.id = LifecyclePreparationId(content_id(&value.canonical_datum())?);
        }
        if !crate::operation_wire::same_datum(&value.canonical_datum(), datum) {
            return Err(OperationError::NonCanonical("lifecycle preparation"));
        }
        Ok(value)
    }
}

fn preparation_datum(dispatch: &FencedDispatchId, binding: &Datum) -> Datum {
    node(
        "lifecycle-preparation-v1",
        vec![
            ("dispatch", id_datum(dispatch.content_id())),
            ("binding", binding.clone()),
        ],
    )
}
