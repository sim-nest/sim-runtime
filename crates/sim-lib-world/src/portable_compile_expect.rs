// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compile record compared with the loaded measure command.

use std::collections::BTreeSet;

use super::OwnedCompile;

const SPEC: (&str, &str, &str, &str, &str, &str, &str) = (
    "checked",
    "same",
    "moved",
    "missing-input",
    "corrupt-input",
    "wrong-target",
    "stub-only",
);

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 1".to_owned(),
        "resident true".to_owned(),
        compile_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([compile_claim(&owned())])
}

pub(super) fn compile_line(row: &OwnedCompile) -> String {
    format!(
        "measure {} {} {} {} {} {} {}",
        row.compile, row.seal, row.lock, row.missing, row.corrupt, row.target, row.stub
    )
}

pub(super) fn compile_claim(row: &OwnedCompile) -> String {
    format!("compile {}", row.compile)
}

fn owned() -> OwnedCompile {
    OwnedCompile {
        compile: SPEC.0.to_owned(),
        seal: SPEC.1.to_owned(),
        lock: SPEC.2.to_owned(),
        missing: SPEC.3.to_owned(),
        corrupt: SPEC.4.to_owned(),
        target: SPEC.5.to_owned(),
        stub: SPEC.6.to_owned(),
    }
}
