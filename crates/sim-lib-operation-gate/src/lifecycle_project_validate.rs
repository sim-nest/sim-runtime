//! Structural validation shared by the strict projector's two read
//! boundaries: an exact closed snapshot, and one prospective next entry.

use std::collections::BTreeSet;

use sim_kernel::Symbol;
use sim_lib_journal::{JournalEntry, JournalObject, VerifiedSnapshot};

use crate::OperationError;

pub(super) fn lifecycle_kind(kind: &Symbol) -> bool {
    matches!(
        kind.as_qualified_str().as_str(),
        "operation/lifecycle-intent-persisted"
            | "operation/lifecycle-cancellation-intent-persisted"
            | "operation/lifecycle-lease-acquired"
            | "operation/lifecycle-dispatch-persisted"
            | "operation/lifecycle-reservation-intent-persisted"
            | "operation/lifecycle-preparation-persisted"
            | "operation/lifecycle-release-intent-persisted"
            | "operation/lifecycle-receipt-persisted"
            | "operation/lifecycle-observation-persisted"
            | "operation/lifecycle-outcome-persisted"
    )
}

pub(super) fn validate_closed_snapshot(snapshot: &VerifiedSnapshot) -> Result<(), OperationError> {
    if snapshot
        .entries()
        .iter()
        .any(|entry| !lifecycle_kind(&entry.kind))
    {
        return Err(OperationError::InvalidTransition(
            "non-lifecycle entry in exact lifecycle snapshot",
        ));
    }
    let referenced = snapshot
        .entries()
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<BTreeSet<_>>();
    if referenced.len() != snapshot.datums().len()
        || snapshot.datums().keys().any(|id| !referenced.contains(id))
    {
        return Err(OperationError::InvalidTransition(
            "unreferenced object in exact lifecycle snapshot",
        ));
    }
    Ok(())
}

pub(super) fn validate_next_entry(
    snapshot: &VerifiedSnapshot,
    entry: &JournalEntry,
    objects: &[JournalObject],
) -> Result<(), OperationError> {
    let (sequence, previous) = match snapshot.head() {
        Some(head) => (
            head.sequence
                .checked_add(1)
                .ok_or(OperationError::SequenceExhausted)?,
            Some(&head.entry),
        ),
        None => (0, None),
    };
    if entry.sequence != sequence
        || entry.previous.as_ref() != previous
        || !lifecycle_kind(&entry.kind)
    {
        return Err(OperationError::InvalidTransition(
            "prospective lifecycle entry is not the exact next entry",
        ));
    }
    let supplied = objects
        .iter()
        .map(|object| &object.id)
        .collect::<BTreeSet<_>>();
    if supplied.len() != objects.len()
        || supplied.iter().any(|id| !entry.payloads.contains(id))
        || entry
            .payloads
            .iter()
            .any(|id| !supplied.contains(id) && !snapshot.datums().contains_key(id))
    {
        return Err(OperationError::InvalidTransition(
            "prospective lifecycle objects do not match its payloads",
        ));
    }
    for object in objects {
        object.verify()?;
    }
    Ok(())
}
