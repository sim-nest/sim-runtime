// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promote record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedPromote};

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    evidence: &'static str,
}

const GAPS: &[GapSpec] = &[
    GapSpec {
        label: "path",
        case: "path",
        reason: "path-source",
        evidence: "",
    },
    GapSpec {
        label: "receipt",
        case: "receipt",
        reason: "local-receipt",
        evidence: "",
    },
    GapSpec {
        label: "model",
        case: "model",
        reason: "model-plan",
        evidence: "",
    },
    GapSpec {
        label: "version",
        case: "version",
        reason: "missing-version",
        evidence: "",
    },
    GapSpec {
        label: "checksum",
        case: "checksum",
        reason: "missing-checksum",
        evidence: "",
    },
    GapSpec {
        label: "source",
        case: "source",
        reason: "missing-source",
        evidence: "",
    },
    GapSpec {
        label: "order",
        case: "order",
        reason: "dependency-order",
        evidence: "",
    },
    GapSpec {
        label: "revision",
        case: "revision",
        reason: "dependency-order",
        evidence: "",
    },
    GapSpec {
        label: "linux",
        case: "linux",
        reason: "linux-requirement",
        evidence: "",
    },
    GapSpec {
        label: "install",
        case: "install",
        reason: "missing-install",
        evidence: "",
    },
    GapSpec {
        label: "unreconciled",
        case: "unreconciled",
        reason: "unreconciled",
        evidence: "",
    },
    GapSpec {
        label: "overlap",
        case: "overlap",
        reason: "overlap",
        evidence: "",
    },
    GapSpec {
        label: "empty",
        case: "empty",
        reason: "missing-package",
        evidence: "",
    },
    GapSpec {
        label: "ci",
        case: "ci",
        reason: "missing-ci",
        evidence: "",
    },
    GapSpec {
        label: "mirror",
        case: "mirror",
        reason: "missing-mirror",
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
        promote_line(&owned_promote()),
    ];
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    claims.insert(promote_claim(&owned_promote()));
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn promote_line(row: &OwnedPromote) -> String {
    format!(
        "promote {} packages {} needs {}",
        shown(&row.case),
        row.packages,
        row.needs,
    )
}

pub(super) fn promote_claim(row: &OwnedPromote) -> String {
    format!(
        "promote {} packages {} needs {}",
        row.case, row.packages, row.needs
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

fn owned_promote() -> OwnedPromote {
    let set = crate::promote::REGISTRY;
    OwnedPromote {
        case: "registry".to_owned(),
        packages: set.artifacts.len().to_string(),
        needs: set.needs.len().to_string(),
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
