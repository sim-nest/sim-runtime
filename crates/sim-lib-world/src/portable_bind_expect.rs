// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Binding record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedBind, OwnedGap};

struct BindSpec {
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

const SERVE: &str = "pure effects distinct same 1";
const SWAPPED: &str = "wrong-binding same 1";
const MISSING: &str = "unimplemented wrong-scope same 1";
const FACET: &str = "pure landing same 1";

const BIND: &[BindSpec] = &[
    BindSpec {
        case: "serve",
        detail: SERVE,
        invokes: "4",
    },
    BindSpec {
        case: "swapped",
        detail: SWAPPED,
        invokes: "1",
    },
    BindSpec {
        case: "missing",
        detail: MISSING,
        invokes: "2",
    },
    BindSpec {
        case: "facet",
        detail: FACET,
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
    for spec in BIND {
        lines.push(bind_line(&owned_bind(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in BIND {
        claims.insert(bind_claim(&owned_bind(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn bind_line(row: &OwnedBind) -> String {
    format!(
        "bind {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn bind_claim(row: &OwnedBind) -> String {
    format!("bind {} {}", row.case, row.detail)
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

fn owned_bind(spec: &BindSpec) -> OwnedBind {
    OwnedBind {
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
