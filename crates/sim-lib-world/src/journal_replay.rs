// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Export one published journal and reopen it on a second instance.
//!
//! The export is the backend state. The second journal publishes those
//! datums and entries. A changed object byte refuses replay.

use std::sync::Arc;

use sim_kernel::Symbol;
use sim_lib_journal::{
    Journal, JournalBackend, JournalEntry, JournalError, JournalHead, JournalObject, MemoryBackend,
    StoredState, replay,
};

use crate::WorldError;

pub(crate) struct Reopened {
    pub(crate) reason: &'static str,
    pub(crate) records: u64,
    pub(crate) head: &'static str,
    pub(crate) corrupt: &'static str,
}

/// Publishes one fact, exports its state, and reopens that state.
///
/// # Errors
/// Returns an error when the published journal refuses the fact, the
/// export, or the second admission.
pub(crate) fn reopen() -> Result<Reopened, WorldError> {
    let backend = Arc::new(MemoryBackend::new());
    let journal = Journal::new(Arc::clone(&backend));
    let lease = journal.acquire_lease().map_err(broke)?;
    let object = JournalObject::from_bytes([9]);
    let fact = JournalEntry::new(
        0,
        None,
        Symbol::qualified("world", "fact"),
        vec![object.id.clone()],
    );
    let head = journal
        .publish(&lease, None, vec![object], vec![fact])
        .map_err(broke)?;
    let exported = backend.read_state().map_err(broke)?;
    let records = u64::try_from(exported.entries.len()).unwrap_or(u64::MAX);
    if records == 0 {
        return Err(WorldError::Qualification(
            "journal replay exported no records".to_owned(),
        ));
    }
    let restored = restore(&exported)?;
    let head_word = if restored == head {
        "matched"
    } else {
        "different"
    };
    let corrupt_word = if corrupt_refuses(&exported) {
        "refused"
    } else {
        "accepted"
    };
    let reason = if head_word == "matched" && corrupt_word == "refused" {
        "reopened"
    } else {
        "same-computation"
    };
    Ok(Reopened {
        reason,
        records,
        head: head_word,
        corrupt: corrupt_word,
    })
}

fn restore(state: &StoredState) -> Result<JournalHead, WorldError> {
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().map_err(broke)?;
    let mut objects = Vec::new();
    for datum in state.datums.values() {
        objects.push(JournalObject::from_datum(datum.clone()).map_err(broke)?);
    }
    let entries = state.entries.values().cloned().collect::<Vec<_>>();
    journal
        .publish(&lease, None, objects, entries)
        .map_err(broke)
}

fn corrupt_refuses(state: &StoredState) -> bool {
    let mut corrupt = state.clone();
    let Some(bytes) = corrupt.objects.values_mut().next() else {
        return false;
    };
    if bytes.is_empty() {
        return false;
    }
    bytes[0] ^= 0xff;
    replay(corrupt).is_err()
}

fn broke(error: JournalError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reopen_exports_bytes_and_refuses_a_corrupt_copy() {
        let reopened = super::reopen().expect("replay");
        assert_eq!(reopened.reason, "reopened");
        assert_eq!(reopened.head, "matched");
        assert_eq!(reopened.corrupt, "refused");
        assert_eq!(reopened.records, 1);
        let source = include_str!("journal_replay.rs");
        assert!(source.contains("read_state()"));
        assert!(source.contains("Journal::new(MemoryBackend::new())"));
        assert!(source.contains("replay(corrupt)"));
    }
}
