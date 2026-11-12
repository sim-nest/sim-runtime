// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Manifest record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedManifest};

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    evidence: &'static str,
}

const GAPS: &[GapSpec] = &[
    GapSpec {
        label: "pin-drift",
        case: "pin-drift",
        reason: "pin-drift",
        evidence: "",
    },
    GapSpec {
        label: "missing-pin",
        case: "missing-pin",
        reason: "missing-pin",
        evidence: "",
    },
    GapSpec {
        label: "private-pin",
        case: "private-pin",
        reason: "private-pin",
        evidence: "",
    },
    GapSpec {
        label: "escape",
        case: "escape",
        reason: "escape",
        evidence: "",
    },
    GapSpec {
        label: "split-source",
        case: "split-source",
        reason: "split-source",
        evidence: "",
    },
    GapSpec {
        label: "overlap",
        case: "overlap",
        reason: "overlap",
        evidence: "",
    },
    GapSpec {
        label: "missing-owner",
        case: "missing-owner",
        reason: "missing-owner",
        evidence: "",
    },
    GapSpec {
        label: "cycle",
        case: "cycle",
        reason: "cycle",
        evidence: "",
    },
    GapSpec {
        label: "linux-target",
        case: "linux-target",
        reason: "linux-target",
        evidence: "",
    },
    GapSpec {
        label: "fabricated",
        case: "fabricated",
        reason: "fabricated",
        evidence: "",
    },
    GapSpec {
        label: "private",
        case: "",
        reason: "private-leakage",
        evidence: "",
    },
];

pub(super) fn expected_deck() -> String {
    let mut lines = vec![
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        manifest_line(&owned_manifest()),
    ];
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    claims.insert(manifest_claim(&owned_manifest()));
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn manifest_line(row: &OwnedManifest) -> String {
    format!(
        "manifest {} owners {} packages {} edges {}",
        shown(&row.case),
        row.owners,
        row.packages,
        row.edges,
    )
}

pub(super) fn manifest_claim(row: &OwnedManifest) -> String {
    format!(
        "manifest {} owners {} packages {} edges {}",
        row.case, row.owners, row.packages, row.edges
    )
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

fn owned_manifest() -> OwnedManifest {
    let table = crate::manifest::OWNER;
    OwnedManifest {
        case: "owner".to_owned(),
        owners: table.owners.len().to_string(),
        packages: table.packages.len().to_string(),
        edges: table.edges.len().to_string(),
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
