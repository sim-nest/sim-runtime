// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored family record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedAuthor, OwnedGap};

const CHANGED: &str = "name,round,stance,path,ratchet,checker,consult";

struct AuthorSpec {
    case: &'static str,
    class: &'static str,
    members: &'static str,
    shown: &'static str,
    lint: &'static str,
    selection: &'static str,
    changed: &'static str,
    round_trip: &'static str,
    shapes: &'static str,
    compiled: &'static str,
    consultation: &'static str,
    bytes: &'static str,
    proof: &'static str,
    phase: &'static str,
    uncertain: &'static str,
    closed: &'static str,
    grounded: &'static str,
    claims: &'static str,
    entries: &'static str,
    postimage: &'static str,
}

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    evidence: &'static str,
}

const AUTHOR: &[AuthorSpec] = &[AuthorSpec {
    case: "selected",
    class: "selected",
    members: "2",
    shown: "chosen",
    lint: "ok",
    selection: "chosen",
    changed: CHANGED,
    round_trip: "true",
    shapes: "distinct",
    compiled: "true",
    consultation: "1",
    bytes: "278",
    proof: "source-deck",
    phase: "authoring",
    uncertain: "false",
    closed: "true",
    grounded: "true",
    claims: "1",
    entries: "1",
    postimage: "278",
}];

const GAPS: &[GapSpec] = &[
    GapSpec {
        label: "consulted",
        case: "consulted",
        reason: "unsatisfied",
        evidence: "semantic-tests",
    },
    GapSpec {
        label: "superseded",
        case: "superseded",
        reason: "unsatisfied",
        evidence: "semantic-tests",
    },
    GapSpec {
        label: "argument",
        case: "argument",
        reason: "missing",
        evidence: "argument",
    },
    GapSpec {
        label: "work",
        case: "work",
        reason: "missing",
        evidence: "work",
    },
    GapSpec {
        label: "gate",
        case: "gate",
        reason: "missing",
        evidence: "gate",
    },
    GapSpec {
        label: "ratchet",
        case: "ratchet",
        reason: "missing",
        evidence: "ratchet",
    },
    GapSpec {
        label: "owner",
        case: "owner",
        reason: "missing",
        evidence: "owner",
    },
    GapSpec {
        label: "checker",
        case: "checker",
        reason: "missing",
        evidence: "checker",
    },
    GapSpec {
        label: "authority",
        case: "authority",
        reason: "authority",
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
    ];
    lines.push(author_line(&owned_author(&AUTHOR[0])));
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in AUTHOR {
        claims.insert(author_claim(&owned_author(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn author_line(row: &OwnedAuthor) -> String {
    format!(
        "author {} {} members {} shown {} lint {} selection {} changed {} round-trip {} shapes {} compiled {} consultation {} bytes {} proof {} phase {} uncertain {} closed {} grounded {} claims {} entries {} postimage {}",
        shown(&row.case),
        shown(&row.class),
        row.members,
        shown(&row.shown),
        shown(&row.lint),
        shown(&row.selection),
        shown(&row.changed),
        shown(&row.round_trip),
        shown(&row.shapes),
        shown(&row.compiled),
        row.consultation,
        row.bytes,
        shown(&row.proof),
        shown(&row.phase),
        shown(&row.uncertain),
        shown(&row.closed),
        shown(&row.grounded),
        row.claims,
        row.entries,
        row.postimage,
    )
}

pub(super) fn author_claim(row: &OwnedAuthor) -> String {
    format!("author {} {} members {}", row.case, row.class, row.members)
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

fn owned_author(spec: &AuthorSpec) -> OwnedAuthor {
    OwnedAuthor {
        case: spec.case.to_owned(),
        class: spec.class.to_owned(),
        members: spec.members.to_owned(),
        shown: spec.shown.to_owned(),
        lint: spec.lint.to_owned(),
        selection: spec.selection.to_owned(),
        changed: spec.changed.to_owned(),
        round_trip: spec.round_trip.to_owned(),
        shapes: spec.shapes.to_owned(),
        compiled: spec.compiled.to_owned(),
        consultation: spec.consultation.to_owned(),
        bytes: spec.bytes.to_owned(),
        proof: spec.proof.to_owned(),
        phase: spec.phase.to_owned(),
        uncertain: spec.uncertain.to_owned(),
        closed: spec.closed.to_owned(),
        grounded: spec.grounded.to_owned(),
        claims: spec.claims.to_owned(),
        entries: spec.entries.to_owned(),
        postimage: spec.postimage.to_owned(),
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
