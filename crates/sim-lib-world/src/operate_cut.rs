// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Journal backend that can stop one admission of an operation lifecycle.
//!
//! The stored entries stay in the memory backend. Clearing the cut lets the
//! next call resume that same journal.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use sim_kernel::{ContentId, Datum};
use sim_lib_journal::{
    Admission, CommitAction, GuardedAdmission, JournalBackend, JournalError, JournalHead,
    JournalObject, Lease, MemoryBackend, StoredDatumRef, StoredState,
};

#[derive(Clone, Copy)]
struct Cut {
    admission: usize,
    after_commit: bool,
}

pub(super) struct CutBackend {
    inner: MemoryBackend,
    admissions: AtomicUsize,
    cut: Mutex<Option<Cut>>,
}

impl CutBackend {
    pub(super) fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            admissions: AtomicUsize::new(0),
            cut: Mutex::new(None),
        }
    }

    pub(super) fn cut(&self, admission: usize, after_commit: bool) {
        self.admissions.store(0, Ordering::SeqCst);
        *self.guard() = Some(Cut {
            admission,
            after_commit,
        });
    }

    pub(super) fn clear(&self) {
        *self.guard() = None;
    }

    fn guard(&self) -> MutexGuard<'_, Option<Cut>> {
        self.cut
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn next_cut(&self) -> (usize, Option<Cut>) {
        let index = self.admissions.fetch_add(1, Ordering::SeqCst) + 1;
        (index, *self.guard())
    }
}

impl JournalBackend for CutBackend {
    fn acquire_lease(&self) -> Result<Lease, JournalError> {
        self.inner.acquire_lease()
    }

    fn read_state(&self) -> Result<StoredState, JournalError> {
        self.inner.read_state()
    }

    fn admit(&self, admission: Admission) -> Result<JournalHead, JournalError> {
        let (index, cut) = self.next_cut();
        if cut.is_some_and(|cut| cut.admission == index && !cut.after_commit) {
            return Err(JournalError::InjectedCrash("before operation boundary"));
        }
        let result = self.inner.admit(admission);
        if result.is_ok() && cut.is_some_and(|cut| cut.admission == index && cut.after_commit) {
            return Err(JournalError::InjectedCrash("after operation boundary"));
        }
        result
    }

    fn admit_then(
        &self,
        admission: Admission,
        action: &mut dyn CommitAction,
    ) -> Result<GuardedAdmission, JournalError> {
        let (index, cut) = self.next_cut();
        if cut.is_some_and(|cut| cut.admission == index && !cut.after_commit) {
            return Err(JournalError::InjectedCrash("before operation boundary"));
        }
        let result = self.inner.admit_then(admission, action);
        if result.is_ok() && cut.is_some_and(|cut| cut.admission == index && cut.after_commit) {
            return Err(JournalError::InjectedCrash("after operation boundary"));
        }
        result
    }

    fn put_datum(&self, object: JournalObject) -> Result<StoredDatumRef, JournalError> {
        self.inner.put_datum(object)
    }

    fn get_datum(&self, meaning: &ContentId) -> Result<Datum, JournalError> {
        self.inner.get_datum(meaning)
    }

    fn rebuild_datum_index(&self) -> Result<Vec<StoredDatumRef>, JournalError> {
        self.inner.rebuild_datum_index()
    }
}
