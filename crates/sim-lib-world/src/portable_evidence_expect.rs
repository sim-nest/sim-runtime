// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Evidence record compared with the loaded why command.

use std::collections::BTreeSet;

use super::OwnedEvidence;

const SPEC: (&str, &str, &str, &str, &str, &str, &str) = (
    "same",
    "moved",
    "same",
    "wrong-domain",
    "revoked-support",
    "missing-object",
    "storage-id-crossing",
);

pub(super) fn expected_deck() -> String {
    [
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        evidence_line(&owned()),
    ]
    .join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([evidence_claim(&owned())])
}

pub(super) fn evidence_line(row: &OwnedEvidence) -> String {
    format!(
        "admit {} {} {} {} {} {} {}",
        row.key, row.receipt, row.telemetry, row.domain, row.revoked, row.missing, row.storage
    )
}

pub(super) fn evidence_claim(row: &OwnedEvidence) -> String {
    format!("evidence {}", row.key)
}

fn owned() -> OwnedEvidence {
    OwnedEvidence {
        key: SPEC.0.to_owned(),
        receipt: SPEC.1.to_owned(),
        telemetry: SPEC.2.to_owned(),
        domain: SPEC.3.to_owned(),
        revoked: SPEC.4.to_owned(),
        missing: SPEC.5.to_owned(),
        storage: SPEC.6.to_owned(),
    }
}
