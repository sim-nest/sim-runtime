// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Export one published journal and reopen it on a second instance.
//!
//! The export is the backend state. The second journal publishes those
//! datums and entries. A second entry references the first object's content
//! id and publishes no new object. Snapshot entries match replay. Telemetry
//! stays in a side sink. A changed object byte refuses replay.

use std::collections::BTreeMap;
use std::sync::Arc;

use sim_conformance_packs::packs::release::{
    ExecutedEntry, ExecutedJournal, format_executed_journal,
};
use sim_kernel::{ContentId, Symbol};
use sim_lib_journal::{
    Journal, JournalBackend, JournalEntry, JournalError, JournalHead, JournalObject, MemoryBackend,
    StoredState, replay,
};

use crate::WorldError;

const TELEMETRY: &str = "side-channel";

pub(crate) struct Reopened {
    pub(crate) reason: &'static str,
    pub(crate) records: u64,
    pub(crate) head: &'static str,
    pub(crate) corrupt: &'static str,
    pub(crate) export: String,
}

/// Publishes one fact, references it, exports that state, and reopens it.
///
/// # Errors
/// Returns an error when the published journal refuses the fact, the
/// export, or the second admission.
pub(crate) fn reopen() -> Result<Reopened, WorldError> {
    let subject = crate::prove::admitted_subject()?;
    let backend = Arc::new(MemoryBackend::new());
    let journal = Journal::new(Arc::clone(&backend));
    let lease = journal.acquire_lease().map_err(broke)?;
    let object = JournalObject::from_bytes(subject.as_bytes().to_vec());
    let kind = Symbol::qualified("world", "fact");
    let first = JournalEntry::new(0, None, kind.clone(), vec![object.id.clone()]);
    let mut head = journal
        .publish(&lease, None, vec![object.clone()], vec![first.clone()])
        .map_err(broke)?;
    let second = JournalEntry::new(1, Some(first.id.clone()), kind, vec![object.id.clone()]);
    head = journal
        .publish(&lease, Some(&head), Vec::new(), vec![second])
        .map_err(broke)?;
    let sink = vec![TELEMETRY];
    let exported = backend.read_state().map_err(broke)?;
    let records = u64::try_from(exported.entries.len()).unwrap_or(u64::MAX);
    let replayed = replay(exported.clone()).map_err(broke)?.collect::<Vec<_>>();
    let again = replay(exported.clone()).map_err(broke)?.collect::<Vec<_>>();
    let snapshot = journal.verified_snapshot().map_err(broke)?;
    let snapshot_ok = snapshot.entries() == replayed.as_slice()
        && replayed
            .iter()
            .all(|entry| entry.payloads.iter().all(|id| snapshot.datum(id).is_some()));
    let shared = shared_reference(&exported);
    let telemetry_outside = sink.len() == 1
        && sink[0] == TELEMETRY
        && !object
            .bytes
            .windows(TELEMETRY.len())
            .any(|window| window == TELEMETRY.as_bytes());
    drop(sink);
    let after = replay(exported.clone()).map_err(broke)?.collect::<Vec<_>>();
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
    let held = snapshot_ok && shared && telemetry_outside && replayed == again && after == replayed;
    let reason = if head_word == "matched" && corrupt_word == "refused" && held {
        "reopened"
    } else {
        "same-computation"
    };
    let export = export_text(&subject, &object, &replayed, TELEMETRY).unwrap_or_default();
    if records == 0 {
        return Err(WorldError::Qualification(
            "journal replay exported no records".to_owned(),
        ));
    }
    Ok(Reopened {
        reason,
        records,
        head: head_word,
        corrupt: corrupt_word,
        export,
    })
}

fn export_text(
    subject: &str,
    object: &JournalObject,
    entries: &[JournalEntry],
    telemetry: &str,
) -> Option<String> {
    let qualified = entries.first()?.kind.as_qualified_str();
    let (namespace, name) = qualified.split_once('/')?;
    let mut rendered = Vec::new();
    for entry in entries {
        if entry.payloads.first() != Some(&object.id) || entry.kind.as_qualified_str() != qualified
        {
            return None;
        }
        rendered.push(ExecutedEntry {
            sequence: entry.sequence,
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            object: 0,
        });
    }
    Some(format_executed_journal(&ExecutedJournal {
        subject: subject.to_owned(),
        objects: vec![object.bytes.clone()],
        entries: rendered,
        telemetry: vec![telemetry.to_owned()],
    }))
}

fn shared_reference(state: &StoredState) -> bool {
    let mut counts = BTreeMap::<ContentId, usize>::new();
    for entry in state.entries.values() {
        for payload in &entry.payloads {
            *counts.entry(payload.clone()).or_insert(0) += 1;
        }
    }
    let Some((id, count)) = counts.iter().find(|(_, count)| **count >= 2) else {
        return false;
    };
    let Some(bytes) = state.objects.get(id) else {
        return false;
    };
    state.objects.values().filter(|item| *item == bytes).count() == 1 && *count >= 2
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
        assert_eq!(reopened.records, 2);
        let subject = crate::prove::admitted_subject().expect("subject");
        assert!(reopened.export.starts_with("journal-export-v1\n"));
        assert!(reopened.export.contains(&format!("subject {subject}\n")));
        assert!(reopened.export.contains("entry 0 "));
        assert!(reopened.export.contains("entry 1 "));
        assert!(reopened.export.contains("telemetry side-channel\n"));
        assert!(!reopened.export.contains("phase/"));
        let source = include_str!("journal_replay.rs");
        assert!(source.contains("read_state()"));
        assert!(source.contains("Journal::new(MemoryBackend::new())"));
        assert!(source.contains("verified_snapshot()"));
        assert!(source.contains("replay(corrupt)"));
    }
}
