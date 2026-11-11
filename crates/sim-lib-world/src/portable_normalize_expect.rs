// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Normalize record compared with the loaded bind command.

use std::collections::BTreeSet;

use super::OwnedNormalize;

const SPEC: (&str, &str, &str, &str, &str, &str, &str, &str, &str) = (
    "normalize",
    "normalized",
    "same",
    "same",
    "moved",
    "moved",
    "refused",
    "refused",
    "refused",
);

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        normalize_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([normalize_claim(&owned())])
}

pub(super) fn normalize_line(row: &OwnedNormalize) -> String {
    format!(
        "{} {} {} {} {} {} {} {} {}",
        row.case,
        row.reason,
        row.alias,
        row.diagnostic,
        row.field,
        row.role,
        row.wrong_role,
        row.old_byte,
        row.authority
    )
}

pub(super) fn normalize_claim(row: &OwnedNormalize) -> String {
    format!("{} {}", row.case, row.reason)
}

fn owned() -> OwnedNormalize {
    OwnedNormalize {
        case: SPEC.0.to_owned(),
        reason: SPEC.1.to_owned(),
        alias: SPEC.2.to_owned(),
        diagnostic: SPEC.3.to_owned(),
        field: SPEC.4.to_owned(),
        role: SPEC.5.to_owned(),
        wrong_role: SPEC.6.to_owned(),
        old_byte: SPEC.7.to_owned(),
        authority: SPEC.8.to_owned(),
    }
}
