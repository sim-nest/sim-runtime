// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Transaction record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedTransact};

struct TransactSpec {
    case: &'static str,
    detail: &'static str,
    invokes: &'static str,
}

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    evidence: &'static str,
}

const APPLY: &str = "applied 1 unchanged same 1";
const REFUSE: &str =
    "wrong-head dirt private-flow generated-owner-bypass stale-base region-escape same 1";
const OVERLAP: &str = "overlap same 1";
const RECOVER: &str = "recovered overlap same 1";
const ROLLBACK: &str = "restored same 1 kept 1";

const TRANSACT: &[TransactSpec] = &[
    TransactSpec {
        case: "apply",
        detail: APPLY,
        invokes: "2",
    },
    TransactSpec {
        case: "refuse",
        detail: REFUSE,
        invokes: "6",
    },
    TransactSpec {
        case: "overlap",
        detail: OVERLAP,
        invokes: "2",
    },
    TransactSpec {
        case: "recover",
        detail: RECOVER,
        invokes: "2",
    },
    TransactSpec {
        case: "rollback",
        detail: ROLLBACK,
        invokes: "2",
    },
];

const GAPS: &[GapSpec] = &[GapSpec {
    label: "private",
    case: "",
    reason: "private-leakage",
    evidence: "",
}];

pub(super) fn expected_deck() -> String {
    let mut lines = vec![
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
    ];
    for spec in TRANSACT {
        lines.push(transact_line(&owned_transact(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in TRANSACT {
        claims.insert(transact_claim(&owned_transact(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn transact_line(row: &OwnedTransact) -> String {
    format!(
        "transact {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn transact_claim(row: &OwnedTransact) -> String {
    format!("transact {} {}", row.case, row.detail)
}

pub(super) fn gap_line(label: &str, row: &OwnedGap) -> String {
    format!(
        "gap {} case {} {} evidence {}",
        label,
        shown(&row.case),
        shown(&row.reason),
        shown(&row.evidence),
    )
}

fn owned_transact(spec: &TransactSpec) -> OwnedTransact {
    OwnedTransact {
        case: spec.case.to_owned(),
        detail: spec.detail.to_owned(),
        invokes: spec.invokes.to_owned(),
    }
}

fn owned_gap(spec: &GapSpec) -> OwnedGap {
    OwnedGap {
        case: spec.case.to_owned(),
        reason: spec.reason.to_owned(),
        evidence: spec.evidence.to_owned(),
    }
}

fn shown(value: &str) -> String {
    if value.is_empty() {
        "blank".to_owned()
    } else if super::private(value) {
        "hidden".to_owned()
    } else {
        value.to_owned()
    }
}
