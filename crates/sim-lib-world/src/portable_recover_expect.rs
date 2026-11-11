// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recovery record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedRecover};

struct RecoverSpec {
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

const STOP: &str = "ambiguous unreconciled unresolved changed same 1";
const ADMIT: &str = "not-retryable unnamed exhausted retry same 1";
const DESCENT: &str = "refiner terminal same 1";
const IMAGE: &str = "unchanged preimage postimage foreign same 1";
const PLAN: &str = "committed apply ambiguous same 1";

const RECOVER: &[RecoverSpec] = &[
    RecoverSpec {
        case: "stop",
        detail: STOP,
        invokes: "8",
    },
    RecoverSpec {
        case: "admit",
        detail: ADMIT,
        invokes: "8",
    },
    RecoverSpec {
        case: "descent",
        detail: DESCENT,
        invokes: "8",
    },
    RecoverSpec {
        case: "image",
        detail: IMAGE,
        invokes: "4",
    },
    RecoverSpec {
        case: "plan",
        detail: PLAN,
        invokes: "5",
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
    for spec in RECOVER {
        lines.push(recover_line(&owned_recover(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in RECOVER {
        claims.insert(recover_claim(&owned_recover(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn recover_line(row: &OwnedRecover) -> String {
    format!(
        "recover {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn recover_claim(row: &OwnedRecover) -> String {
    format!("recover {} {}", row.case, row.detail)
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

fn owned_recover(spec: &RecoverSpec) -> OwnedRecover {
    OwnedRecover {
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
