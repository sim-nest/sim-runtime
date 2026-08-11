use std::collections::{BTreeMap, BTreeSet};

use sim_lib_mutation::ManagedId;

/// Opaque, language-neutral notification that one unreachable target is ready.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizationRecord {
    /// Collector registration identity.
    pub registration: u64,
    /// Unreachable managed identity; it is not a live handle.
    pub target: ManagedId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationState {
    Registered,
    Cancelled,
    Admitted,
}

/// At-most-once finalization registrations, separate from managed storage.
#[derive(Default)]
pub struct FinalizationRegistry {
    next: u64,
    entries: BTreeMap<u64, (ManagedId, RegistrationState)>,
}

impl FinalizationRegistry {
    /// Registers a target and returns a stable cancellation identity.
    pub fn register(&mut self, target: ManagedId) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.entries
            .insert(id, (target, RegistrationState::Registered));
        id
    }
    /// Cancels a pending registration. Cancellation is idempotent.
    pub fn cancel(&mut self, registration: u64) -> bool {
        let Some((_, state)) = self.entries.get_mut(&registration) else {
            return false;
        };
        if *state != RegistrationState::Registered {
            return false;
        }
        *state = RegistrationState::Cancelled;
        true
    }
    pub(crate) fn ready(&self, swept: &BTreeSet<ManagedId>) -> Vec<FinalizationRecord> {
        self.entries
            .iter()
            .filter_map(|(&registration, &(target, state))| {
                (state == RegistrationState::Registered && swept.contains(&target)).then_some(
                    FinalizationRecord {
                        registration,
                        target,
                    },
                )
            })
            .collect()
    }
    pub(crate) fn mark_admitted(&mut self, records: &[FinalizationRecord]) {
        for record in records {
            if let Some((_, state)) = self.entries.get_mut(&record.registration) {
                *state = RegistrationState::Admitted;
            }
        }
    }
}
