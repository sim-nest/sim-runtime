// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Qualify record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedQualify};

struct QualifySpec {
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

const COMPOSITION: &str = "pages 1 revision 10e8d81a27f696c6d1896b44d6327c2c7bf92b54c5ac2f938455c68787cd0b92 transitions 1 stale-effects 0 effects 1 hot-delta 0 samples 20 providers model/direction,model/landing,model/snapshot stale-edges 1 bound 1 proof 0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e linux 0";
const BEGIN: &str = "distinct begin-required landed 1 stale 1 proof 0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e";

const QUALIFY: &[QualifySpec] = &[
    QualifySpec {
        case: "composition",
        detail: COMPOSITION,
        invokes: "1",
    },
    QualifySpec {
        case: "linux",
        detail: "linux-provider",
        invokes: "0",
    },
    QualifySpec {
        case: "freeze",
        detail: "freeze-mismatch empty freeze-mismatch",
        invokes: "0",
    },
    QualifySpec {
        case: "advice",
        detail: "advice-rejected",
        invokes: "0",
    },
    QualifySpec {
        case: "begin",
        detail: BEGIN,
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
    for spec in QUALIFY {
        lines.push(qualify_line(&owned_qualify(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in QUALIFY {
        claims.insert(qualify_claim(&owned_qualify(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn qualify_line(row: &OwnedQualify) -> String {
    format!(
        "qualify {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn qualify_claim(row: &OwnedQualify) -> String {
    format!("qualify {} {}", row.case, row.detail)
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

fn owned_qualify(spec: &QualifySpec) -> OwnedQualify {
    OwnedQualify {
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
