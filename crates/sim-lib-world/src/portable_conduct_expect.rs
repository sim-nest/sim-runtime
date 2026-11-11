// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conduct record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedConduct, OwnedGap};

struct ConductSpec {
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

const QUIET: &str = "quiescent invokes 0 journal same state planned";
const ONE: &str = "advanced invokes 1 inspected still state running";
const OFFER: &str =
    "blocked quiescent zero 0 wake resource-offer advanced quiescent invokes 1 leases empty";
const DIRECTION: &str = "need-direction need-direction journal same wrong-direction advisory-direction stale-direction journal same direction-authority advanced state planned quiescent journal same replayed-direction advanced invokes 1 state running";

const CONDUCT: &[ConductSpec] = &[
    ConductSpec {
        case: "quiet",
        detail: QUIET,
        invokes: "0",
    },
    ConductSpec {
        case: "one",
        detail: ONE,
        invokes: "1",
    },
    ConductSpec {
        case: "offer",
        detail: OFFER,
        invokes: "1",
    },
    ConductSpec {
        case: "direction",
        detail: DIRECTION,
        invokes: "1",
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
    for spec in CONDUCT {
        lines.push(conduct_line(&owned_conduct(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in CONDUCT {
        claims.insert(conduct_claim(&owned_conduct(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn conduct_line(row: &OwnedConduct) -> String {
    format!(
        "conduct {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn conduct_claim(row: &OwnedConduct) -> String {
    format!("conduct {} {}", row.case, row.detail)
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

fn owned_conduct(spec: &ConductSpec) -> OwnedConduct {
    OwnedConduct {
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
