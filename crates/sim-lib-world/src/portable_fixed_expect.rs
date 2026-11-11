// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed-point record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedFixed, OwnedGap};

struct FixedSpec {
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

const AGAIN: &str = "sealed 2 zero same 1";
const DELETE: &str = "closure claim/alpha,claim/extra,fact/alpha,fact/extra retained fact/beta";
const GAP_DETAIL: &str = "proof-failed claim/alpha,claim/beta";
const OSCILLATE: &str = "oscillation fact/alpha same 1";
const RETIRE: &str = "retired 1 duplicate 0";

const FIXED: &[FixedSpec] = &[
    FixedSpec {
        case: "again",
        detail: AGAIN,
        invokes: "4",
    },
    FixedSpec {
        case: "delete",
        detail: DELETE,
        invokes: "3",
    },
    FixedSpec {
        case: "gap",
        detail: GAP_DETAIL,
        invokes: "3",
    },
    FixedSpec {
        case: "oscillate",
        detail: OSCILLATE,
        invokes: "5",
    },
    FixedSpec {
        case: "retire",
        detail: RETIRE,
        invokes: "3",
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
    for spec in FIXED {
        lines.push(fixed_line(&owned_fixed(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in FIXED {
        claims.insert(fixed_claim(&owned_fixed(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn fixed_line(row: &OwnedFixed) -> String {
    format!(
        "fixed {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn fixed_claim(row: &OwnedFixed) -> String {
    format!("fixed {} {}", row.case, row.detail)
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

fn owned_fixed(spec: &FixedSpec) -> OwnedFixed {
    OwnedFixed {
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
