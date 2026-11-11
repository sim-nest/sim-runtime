// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One opened execution compared through the published journal.

use std::collections::BTreeSet;
use std::sync::Arc;

use sim_kernel::{ContentId, Symbol};
use sim_lib_roadmap_runner::{
    DeltaAppend, ExecutionJournal, ExecutionPins, Limits, MemoryBackend, SemanticChange,
};

use crate::WorldError;

pub(crate) struct Compared {
    pub(crate) reason: &'static str,
    pub(crate) records: u64,
}

/// Opens one execution, appends one fact, and compares the two rebuilds.
///
/// # Errors
/// Returns an error when the published journal refuses the log or either
/// rebuild fails. Equal results are the same computation.
pub(crate) fn compare() -> Result<Compared, WorldError> {
    let journal = ExecutionJournal::new(
        Arc::new(MemoryBackend::new()),
        "world-journal",
        Limits::default(),
    );
    let opened = journal
        .open(pins(), None)
        .map_err(|_| WorldError::Qualification("journal compare open failed".to_owned()))?;
    let appended = journal
        .append_delta(&opened.head, "fact/change", vec![change()], BTreeSet::new())
        .map_err(|_| WorldError::Qualification("journal compare append failed".to_owned()))?;
    let DeltaAppend::Appended(_) = appended else {
        return Err(WorldError::Qualification(
            "journal compare appended nothing".to_owned(),
        ));
    };
    let rebuilt = journal
        .rebuild()
        .map_err(|_| WorldError::Qualification("journal compare rebuild failed".to_owned()))?;
    let reported = journal
        .rebuild_report()
        .map_err(|_| WorldError::Qualification("journal compare report failed".to_owned()))?;
    let records = u64::try_from(rebuilt.records.len()).unwrap_or(u64::MAX);
    if records == 0 {
        return Err(WorldError::Qualification(
            "journal compare saw no records".to_owned(),
        ));
    }
    let reason = if rebuilt == reported {
        "same-computation"
    } else {
        "distinct-computation"
    };
    Ok(Compared { reason, records })
}

fn pins() -> ExecutionPins {
    ExecutionPins {
        conduct: "world-journal".to_owned(),
        policy: "sealed".to_owned(),
        source_deck: ContentId::from_bytes(Symbol::qualified("deck", "sha256-v1"), [2; 32]),
        model_pick: "none".to_owned(),
        runner_generation: "roadmap-runner".to_owned(),
    }
}

fn change() -> SemanticChange {
    SemanticChange {
        key: "fact/000000".to_owned(),
        before: None,
        after: Some(ContentId::from_bytes(
            Symbol::qualified("deck", "sha256-v1"),
            [4; 32],
        )),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compare_calls_both_rebuilds_before_the_reason() {
        let source = include_str!("journal_drive.rs");
        let rebuild_at = source.find(".rebuild()").expect("rebuild call");
        let report_at = source.find(".rebuild_report()").expect("report call");
        let compare_at = source.find("rebuilt == reported").expect("comparison");
        assert!(rebuild_at < report_at);
        assert!(report_at < compare_at);
        assert!(source.contains("rebuilt.records.len()"));
        assert!(source.contains("\"same-computation\""));
        assert!(source.contains("\"distinct-computation\""));
    }
}
