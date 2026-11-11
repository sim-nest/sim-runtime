// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Landing record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedLand};

struct LandSpec {
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

const FOREIGN: &str = "would-commit applied kept same 1";
const REPLAY: &str = "already-true same 1";
const RECOVER: &str = "restored exact kept 1";
const GENERATED: &str = "generated-owner-bypass same 1";
const CACHE: &str = "unknown current stale same 1";
const STATUS: &str = "present missing same 1";

const LAND: &[LandSpec] = &[
    LandSpec {
        case: "foreign",
        detail: FOREIGN,
        invokes: "3",
    },
    LandSpec {
        case: "replay",
        detail: REPLAY,
        invokes: "2",
    },
    LandSpec {
        case: "recover",
        detail: RECOVER,
        invokes: "3",
    },
    LandSpec {
        case: "generated",
        detail: GENERATED,
        invokes: "2",
    },
    LandSpec {
        case: "cache",
        detail: CACHE,
        invokes: "6",
    },
    LandSpec {
        case: "status",
        detail: STATUS,
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
    for spec in LAND {
        lines.push(land_line(&owned_land(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in LAND {
        claims.insert(land_claim(&owned_land(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn land_line(row: &OwnedLand) -> String {
    format!(
        "land {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn land_claim(row: &OwnedLand) -> String {
    format!("land {} {}", row.case, row.detail)
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

fn owned_land(spec: &LandSpec) -> OwnedLand {
    OwnedLand {
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
