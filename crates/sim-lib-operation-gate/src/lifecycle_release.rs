//! Durable release intent and short, owner-local guarded admission.

use crate::operation_wire::{content_id, field, id_datum, id_from_datum, node, node_fields};
use crate::{
    FencedDispatch, FencedDispatchId, LeaseClock, LifecyclePreparation, LifecyclePreparationId,
    OperationError, OperationLease, OperationLeaseId,
};
use sim_kernel::{ContentId, Datum, Symbol};
pub use sim_lib_journal::AdmissionDisposition as ReleaseDisposition;
use sim_lib_journal::{Journal, JournalBackend, JournalEntry, JournalObject, Lease};
use std::sync::Arc;

/// Canonical intent to release one already-prepared dispatch, not proof of start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleRelease {
    id: ContentId,
    dispatch: FencedDispatchId,
    preparation: LifecyclePreparationId,
    lease: OperationLeaseId,
}

impl LifecycleRelease {
    fn new(
        dispatch: FencedDispatchId,
        preparation: LifecyclePreparationId,
        lease: OperationLeaseId,
    ) -> Result<Self, OperationError> {
        let mut value = Self {
            id: dispatch.content_id().clone(),
            dispatch,
            preparation,
            lease,
        };
        value.id = content_id(&value.canonical_datum())?;
        Ok(value)
    }
    /// Returns this release intent's semantic identity.
    pub const fn id(&self) -> &ContentId {
        &self.id
    }
    /// Returns the exact dispatch correlated with this intent.
    pub const fn dispatch(&self) -> &FencedDispatchId {
        &self.dispatch
    }
    /// Returns the durable reservation selected for release.
    pub const fn preparation(&self) -> &LifecyclePreparationId {
        &self.preparation
    }
    /// Returns the bounded, clock-scoped operation lease selected at release.
    pub const fn lease(&self) -> &OperationLeaseId {
        &self.lease
    }
    /// Returns the canonical journal payload.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "lifecycle-release-intent-v1",
            vec![
                ("dispatch", id_datum(self.dispatch.content_id())),
                ("preparation", id_datum(self.preparation.content_id())),
                ("lease", id_datum(self.lease.content_id())),
            ],
        )
    }
    pub(super) fn from_datum(datum: &Datum) -> Result<Self, OperationError> {
        let fields = node_fields(datum, "lifecycle-release-intent-v1", 3)?;
        let value = Self::new(
            FencedDispatchId(id_from_datum(field(fields, "dispatch")?)?),
            LifecyclePreparationId(id_from_datum(field(fields, "preparation")?)?),
            OperationLeaseId(id_from_datum(field(fields, "lease")?)?),
        )?;
        if !crate::operation_wire::same_datum(&value.canonical_datum(), datum) {
            return Err(OperationError::NonCanonical("release intent"));
        }
        Ok(value)
    }
}

/// Short native-owner action receiving the exact durably admitted release.
///
/// The supplied record binds dispatch, preparation and lease without reopening
/// the journal from inside its exclusive commit guard. It is correlation data;
/// live resource custody and cancellation still belong to the action's owner.
/// Implementations must not access this journal, enqueue a deferred release or
/// wait for payload completion. Retain failures for independent reconciliation.
pub trait ReleaseAction {
    /// Performs the short release step under the active journal/fence guard.
    fn after_commit(&mut self, release: &LifecycleRelease);
}

impl<F: FnMut(&LifecycleRelease)> ReleaseAction for F {
    fn after_commit(&mut self, release: &LifecycleRelease) {
        self(release);
    }
}

/// Borrowed release authority, usable only at the actual resource-owning gate.
///
/// The gate action must be short and synchronous in the native owner. It cannot
/// enqueue a release that may arrive after this call returns, wait for payload
/// completion, or mutate this journal. Cancellation and release are serialized
/// inside that same native gate. Persisted intent does not prove payload start.
pub trait PreparedReleaseAdmission {
    /// Borrows the exact preparation whose live admission this owner will check.
    /// This is correlation, not authority: the native owner must match its own
    /// retained preparation and check the same ID again in the guarded callback.
    /// Unsupported implementations provide no binding and cannot qualify native
    /// prepared release. The getter does not query or mutate the journal.
    fn preparation(&self) -> Option<&LifecyclePreparation> {
        None
    }

    /// Commits release intent and invokes the gate under live fence/clock checks.
    /// Any error or lost acknowledgement requires observation, never blind replay.
    fn admit(
        &mut self,
        action: &mut dyn ReleaseAction,
    ) -> Result<ReleaseDisposition, OperationError>;
}

pub(super) struct PreparedAdmission<'a, B: JournalBackend> {
    pub journal: &'a Journal<Arc<B>>,
    pub writer: &'a Lease,
    pub clock: Option<&'a dyn LeaseClock>,
    pub lease: &'a OperationLease,
    pub dispatch: &'a FencedDispatch,
    pub preparation: &'a LifecyclePreparation,
    pub attempted: bool,
}

impl<B: JournalBackend> PreparedReleaseAdmission for PreparedAdmission<'_, B> {
    fn preparation(&self) -> Option<&LifecyclePreparation> {
        Some(self.preparation)
    }

    fn admit(
        &mut self,
        action: &mut dyn ReleaseAction,
    ) -> Result<ReleaseDisposition, OperationError> {
        if std::mem::replace(&mut self.attempted, true) {
            return Err(OperationError::InvalidTransition(
                "release admission requires reconciliation",
            ));
        }
        let clock = self.clock.ok_or(OperationError::InvalidLease)?;
        let domain = self.lease.clock().ok_or(OperationError::InvalidLease)?;
        let snapshot = self.journal.verified_snapshot()?;
        let expected = snapshot.head().cloned();
        let records = crate::lifecycle_project::project(snapshot.clone())?;
        let record = records
            .get(self.dispatch.operation())
            .ok_or(OperationError::NotResumed)?;
        if record.dispatches().last().map(|value| value.id()) != Some(self.dispatch.id())
            || record.preparations().last().map(|value| value.id()) != Some(self.preparation.id())
            || record.leases().last().map(|value| value.id()) != Some(self.lease.id())
            || self.lease.fence() != self.writer.fence()
            || record.last_step() != crate::OperationStep::PreparationPersisted
            || record
                .releases()
                .iter()
                .any(|value| value.dispatch() == self.dispatch.id())
        {
            return Err(OperationError::InvalidTransition(
                "release binding is not current",
            ));
        }
        let release = LifecycleRelease::new(
            self.dispatch.id().clone(),
            self.preparation.id().clone(),
            self.lease.id().clone(),
        )?;
        let object = JournalObject::from_datum(release.canonical_datum())?;
        let sequence = expected
            .as_ref()
            .map_or(Some(0), |head| head.sequence.checked_add(1))
            .ok_or(OperationError::SequenceExhausted)?;
        let entry = JournalEntry::new(
            sequence,
            expected.as_ref().map(|head| head.entry.clone()),
            Symbol::qualified("operation", "lifecycle-release-intent-persisted"),
            vec![object.id.clone()],
        );
        let mut refusal = None;
        crate::lifecycle_project::validate_extension(
            &snapshot,
            &entry,
            std::slice::from_ref(&object),
        )?;
        let mut gate = || match clock.read() {
            Ok(reading)
                if crate::operation_wire::same_datum(&reading.domain, domain)
                    && self.lease.is_live_at(reading.tick) =>
            {
                action.after_commit(&release)
            }
            Ok(_) => refusal = Some(OperationError::InvalidLease),
            Err(error) => refusal = Some(error),
        };
        let result = self.journal.publish_then(
            self.writer,
            expected.as_ref(),
            vec![object],
            vec![entry],
            &mut gate,
        )?;
        if let Some(error) = refusal {
            return Err(error);
        }
        Ok(result.disposition)
    }
}
