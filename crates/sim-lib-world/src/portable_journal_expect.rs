// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Replay record compared with the loaded journal command.

use std::collections::BTreeSet;

use super::OwnedReplay;

const SPEC: (&str, &str, &str, &str, &str) = ("replay", "reopened", "matched", "refused", "1");

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        replay_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([replay_claim(&owned())])
}

pub(super) fn replay_line(row: &OwnedReplay) -> String {
    format!(
        "{} {} {} {} records {}",
        row.case, row.reason, row.head, row.corrupt, row.records
    )
}

pub(super) fn replay_claim(row: &OwnedReplay) -> String {
    format!("{} {}", row.case, row.reason)
}

fn owned() -> OwnedReplay {
    OwnedReplay {
        case: SPEC.0.to_owned(),
        reason: SPEC.1.to_owned(),
        head: SPEC.2.to_owned(),
        corrupt: SPEC.3.to_owned(),
        records: SPEC.4.to_owned(),
    }
}
