//! Sticky cancellation intent and a writer-scoped control handle.

use std::sync::Arc;

use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::{Journal, JournalBackend, JournalEntry, JournalHead, JournalObject, Lease};

use crate::operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields};
use crate::{OperationError, OperationGrantId, OperationId};

const KIND: &str = "lifecycle-cancellation-intent-persisted";

/// Durable request to stop an operation, not evidence of termination or cleanup.
/// Cancellation is orthogonal to execution progress: outstanding preparation,
/// receipts and observations retain their exact bindings after this request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleCancellation {
    id: ContentId,
    operation: OperationId,
    grant: OperationGrantId,
    reason: Datum,
}

impl LifecycleCancellation {
    pub(super) fn new(
        operation: OperationId,
        grant: OperationGrantId,
        reason: Datum,
    ) -> Result<Self, OperationError> {
        if reason == Datum::Nil {
            return Err(OperationError::NonCanonical("empty cancellation reason"));
        }
        let mut value = Self {
            id: operation.content_id().clone(),
            operation,
            grant,
            reason,
        };
        value.id = content_id(&value.canonical_datum())?;
        Ok(value)
    }
    /// Returns the identified durable intent, not a native stop receipt.
    pub const fn id(&self) -> &ContentId {
        &self.id
    }
    /// Returns the exact operation whose further execution is prohibited.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }
    /// Returns the operation's installed grant binding, not caller authority.
    pub const fn grant(&self) -> &OperationGrantId {
        &self.grant
    }
    /// Returns the first accepted reason; redelivery never replaces it.
    pub const fn reason(&self) -> &Datum {
        &self.reason
    }
    /// Returns canonical intent data with no transferable control authority.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "lifecycle-cancellation-intent-v1",
            vec![
                ("operation", id_datum(self.operation.content_id())),
                ("grant", id_datum(self.grant.content_id())),
                ("reason", self.reason.clone()),
            ],
        )
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, "lifecycle-cancellation-intent-v1", 3)?;
        let value = Self::new(
            OperationId(id_from_datum(field(fields, "operation")?)?),
            OperationGrantId(id_from_datum(field(fields, "grant")?)?),
            field(fields, "reason")?.clone(),
        )?;
        if !crate::operation_wire::same_datum(&value.canonical_datum(), datum) {
            return Err(OperationError::NonCanonical("cancellation intent"));
        }
        Ok(value)
    }
}

/// Short, monotonic notification to the exact accepted operation's native owner.
/// Implementations must not wait, access this journal, or reuse this signal for
/// another accepted run. Notification is not termination or resource disposition.
pub trait CancellationSignal: Send + Sync {
    /// Requests stop of the bound operation without waiting for its execution.
    fn request_stop(&self);
}

/// Independently usable control handle sharing one accepted writer generation.
/// It neither borrows the running performer nor acquires a new journal fence.
/// Only a trusted owner holding this handle may request cancellation; decoding
/// an operation id, grant string or socket message does not construct a handle.
pub struct CancellationHandle<B: JournalBackend> {
    pub(super) journal: Arc<Journal<Arc<B>>>,
    pub(super) writer: Lease,
    pub(super) operation: OperationId,
    pub(super) grant: OperationGrantId,
    pub(super) signal: Arc<dyn CancellationSignal>,
}

impl<B: JournalBackend> Clone for CancellationHandle<B> {
    fn clone(&self) -> Self {
        Self {
            journal: self.journal.clone(),
            writer: self.writer.clone(),
            operation: self.operation.clone(),
            grant: self.grant.clone(),
            signal: self.signal.clone(),
        }
    }
}

impl<B: JournalBackend> CancellationHandle<B> {
    /// Persists a sticky request and notifies the exact native owner.
    /// The return value confirms intent only. Any publication/acknowledgement
    /// error requires reconciliation; no error claims that a request was absent.
    /// Concurrent head changes require a fresh checked request, not blind rebase.
    pub fn request(&self, reason: Datum) -> Result<LifecycleCancellation, OperationError> {
        let snapshot = self.journal.verified_snapshot()?;
        let records = crate::lifecycle_project::project(snapshot.clone())?;
        let record = records
            .get(&self.operation)
            .ok_or(OperationError::NotResumed)?;
        if record.grant().id() != &self.grant {
            return Err(OperationError::GrantMismatch);
        }
        let (cancellation, expected, entry, objects) = if let Some(existing) = record.cancellation()
        {
            // Exact batch redelivery checks the old writer fence even when the
            // durable request is already present. It cannot mint new authority.
            let entry = snapshot
                .entries()
                .iter()
                .find(|entry| {
                    entry.kind == Symbol::qualified("operation", KIND)
                        && entry.payloads.as_slice() == std::slice::from_ref(existing.id())
                })
                .ok_or(OperationError::NonCanonical("missing cancellation entry"))?;
            let expected = entry
                .sequence
                .checked_sub(1)
                .map(|sequence| {
                    let previous = snapshot
                        .entries()
                        .iter()
                        .find(|entry| entry.sequence == sequence)
                        .ok_or(OperationError::NonCanonical(
                            "missing cancellation predecessor",
                        ))?;
                    Ok::<_, OperationError>(JournalHead {
                        sequence,
                        entry: previous.id.clone(),
                    })
                })
                .transpose()?;
            (existing.clone(), expected, entry.clone(), vec![])
        } else {
            let cancellation =
                LifecycleCancellation::new(self.operation.clone(), self.grant.clone(), reason)?;
            let object = JournalObject::from_datum(cancellation.canonical_datum())?;
            let expected = snapshot.head().cloned();
            let sequence = expected
                .as_ref()
                .map_or(Some(0), |head| head.sequence.checked_add(1))
                .ok_or(OperationError::SequenceExhausted)?;
            let entry = JournalEntry::new(
                sequence,
                expected.as_ref().map(|head| head.entry.clone()),
                Symbol::qualified("operation", KIND),
                vec![object.id.clone()],
            );
            let objects = vec![object];
            crate::lifecycle_project::validate_extension(&snapshot, &entry, &objects)?;
            (cancellation, expected, entry, objects)
        };
        let result = self.journal.publish_then(
            &self.writer,
            expected.as_ref(),
            objects,
            vec![entry],
            &mut || self.signal.request_stop(),
        )?;
        if result.disposition
            == sim_lib_journal::AdmissionDisposition::AlreadyCommittedActionNotInvoked
        {
            // A stop signal is monotonic and scoped to this accepted operation,
            // unlike a payload-release action. Restore it on exact redelivery.
            self.signal.request_stop();
        }
        Ok(cancellation)
    }
}
