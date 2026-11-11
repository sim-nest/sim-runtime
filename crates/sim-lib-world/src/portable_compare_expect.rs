// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cold comparison compared with the loaded prove command.

use std::collections::BTreeSet;

use super::OwnedCompare;

const SPEC: (&str, &str, &str, &str, &str, &str, &str) =
    ("28", "disabled", "0", "10000", "9500", "7000", "40");

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        compare_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([compare_claim(&owned())])
}

pub(super) fn compare_line(row: &OwnedCompare) -> String {
    format!(
        "compare {} {} {} {} {} {} {}",
        row.leaves, row.reuse, row.false_reuse, row.d1, row.d2, row.d4, row.assay
    )
}

pub(super) fn compare_claim(row: &OwnedCompare) -> String {
    format!("compare {}", row.leaves)
}

fn owned() -> OwnedCompare {
    OwnedCompare {
        leaves: SPEC.0.to_owned(),
        reuse: SPEC.1.to_owned(),
        false_reuse: SPEC.2.to_owned(),
        d1: SPEC.3.to_owned(),
        d2: SPEC.4.to_owned(),
        d4: SPEC.5.to_owned(),
        assay: SPEC.6.to_owned(),
    }
}
