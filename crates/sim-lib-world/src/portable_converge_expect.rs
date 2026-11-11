// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Converge record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedConverge, OwnedGap};

struct ConvergeSpec {
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

const PRESERVE: &str = "self-approval held 1 uncertain 1 same 1 later uncertain";
const INVALIDATE: &str = "affected a closed b changed 1";
const OSCILLATE: &str = "empty 1 seen note same 1";
const PARALLEL: &str = "same 1 failures a:proof-failed,b:proof-failed";
const HONEST: &str = "incomplete need malformed bad same 1";

const CONVERGE: &[ConvergeSpec] = &[
    ConvergeSpec {
        case: "preserve",
        detail: PRESERVE,
        invokes: "5",
    },
    ConvergeSpec {
        case: "invalidate",
        detail: INVALIDATE,
        invokes: "3",
    },
    ConvergeSpec {
        case: "oscillate",
        detail: OSCILLATE,
        invokes: "5",
    },
    ConvergeSpec {
        case: "parallel",
        detail: PARALLEL,
        invokes: "2",
    },
    ConvergeSpec {
        case: "honest",
        detail: HONEST,
        invokes: "4",
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
    for spec in CONVERGE {
        lines.push(converge_line(&owned_converge(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in CONVERGE {
        claims.insert(converge_claim(&owned_converge(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn converge_line(row: &OwnedConverge) -> String {
    format!(
        "converge {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn converge_claim(row: &OwnedConverge) -> String {
    format!("converge {} {}", row.case, row.detail)
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

fn owned_converge(spec: &ConvergeSpec) -> OwnedConverge {
    OwnedConverge {
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
