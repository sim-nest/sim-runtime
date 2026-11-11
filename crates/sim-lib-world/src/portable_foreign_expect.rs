// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Foreign record compared with the loaded succeed command.

use std::collections::BTreeSet;

use super::OwnedForeign;

const SPEC: (&str, &str, &str) = ("portable", "portable", "private-branch");

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        foreign_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([foreign_claim(&owned())])
}

pub(super) fn foreign_line(row: &OwnedForeign) -> String {
    format!(
        "lantern {} harbor {} {}",
        row.specimen, row.second, row.branch
    )
}

pub(super) fn foreign_claim(row: &OwnedForeign) -> String {
    format!("foreign {} {}", row.specimen, row.second)
}

fn owned() -> OwnedForeign {
    OwnedForeign {
        specimen: SPEC.0.to_owned(),
        second: SPEC.1.to_owned(),
        branch: SPEC.2.to_owned(),
    }
}
