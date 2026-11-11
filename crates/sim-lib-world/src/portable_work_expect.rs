// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored work record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedWork};

const PATH: &str = "crates/widget/src/lib.rs";
const ROLE: &str = "funded-target";
const STATUS: &str = "declared";
const RECEIVER: &str = "work/api/projection";

struct WorkSpec {
    case: &'static str,
    class: &'static str,
    bytes: &'static str,
    files: &'static str,
    tokens: &'static str,
    path: &'static str,
    role: &'static str,
    status: &'static str,
    receiver: &'static str,
    packet: &'static str,
    proposal: &'static str,
    partial: &'static str,
    attempts: &'static str,
    reads: &'static str,
    workers: &'static str,
    proposals: &'static str,
}

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    reads: &'static str,
    attempts: &'static str,
    evidence: &'static str,
    mirrors: &'static str,
    workers: &'static str,
    proposals: &'static str,
}

const WORK: &[WorkSpec] = &[
    WorkSpec {
        case: "admitted",
        class: "admitted",
        bytes: "12",
        files: "1",
        tokens: "12",
        path: PATH,
        role: ROLE,
        status: STATUS,
        receiver: RECEIVER,
        packet: "present",
        proposal: "mapped",
        partial: "",
        attempts: "1",
        reads: "1",
        workers: "1",
        proposals: "1",
    },
    WorkSpec {
        case: "incomplete",
        class: "incomplete",
        bytes: "12",
        files: "1",
        tokens: "12",
        path: PATH,
        role: ROLE,
        status: STATUS,
        receiver: RECEIVER,
        packet: "present",
        proposal: "",
        partial: "partial-map",
        attempts: "1",
        reads: "1",
        workers: "1",
        proposals: "1",
    },
];

const GAPS: &[GapSpec] = &[
    GapSpec {
        label: "oversize",
        case: "oversize",
        reason: "oversize",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "",
        workers: "0",
        proposals: "0",
    },
    GapSpec {
        label: "malformed",
        case: "malformed",
        reason: "malformed-bound",
        reads: "1",
        attempts: "2",
        evidence: "second",
        mirrors: "",
        workers: "2",
        proposals: "2",
    },
    GapSpec {
        label: "descent",
        case: "descent",
        reason: "no-descent",
        reads: "1",
        attempts: "0",
        evidence: "same",
        mirrors: "",
        workers: "1",
        proposals: "1",
    },
    GapSpec {
        label: "namespace",
        case: "namespace",
        reason: "namespace",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "true",
        workers: "0",
        proposals: "0",
    },
    GapSpec {
        label: "role",
        case: "role",
        reason: "role",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "true",
        workers: "0",
        proposals: "0",
    },
    GapSpec {
        label: "stale",
        case: "stale",
        reason: "stale-copy",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "true",
        workers: "0",
        proposals: "0",
    },
    GapSpec {
        label: "schema",
        case: "schema",
        reason: "schema",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "true",
        workers: "0",
        proposals: "0",
    },
    GapSpec {
        label: "effects",
        case: "effects",
        reason: "effects-or-approval",
        reads: "1",
        attempts: "0",
        evidence: "",
        mirrors: "",
        workers: "1",
        proposals: "1",
    },
    GapSpec {
        label: "private",
        case: "",
        reason: "private-leakage",
        reads: "0",
        attempts: "0",
        evidence: "",
        mirrors: "",
        workers: "0",
        proposals: "0",
    },
];

pub(super) fn expected_deck() -> String {
    let mut lines = vec![
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
    ];
    lines.push(work_line(&owned_work(&WORK[0])));
    lines.push(gap_line(GAPS[0].label, &owned_gap(&GAPS[0])));
    lines.push(work_line(&owned_work(&WORK[1])));
    for gap in GAPS.iter().skip(1) {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in WORK {
        claims.insert(work_claim(&owned_work(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn work_line(row: &OwnedWork) -> String {
    format!(
        "work {} {} bytes {} files {} tokens {} path {} role {} status {} receiver {} packet {} proposal {} partial {} attempts {} reads {} workers {} proposals {}",
        shown(&row.case),
        shown(&row.class),
        row.bytes,
        row.files,
        row.tokens,
        shown(&row.path),
        shown(&row.role),
        shown(&row.status),
        shown(&row.receiver),
        shown(&row.packet),
        shown(&row.proposal),
        shown(&row.partial),
        row.attempts,
        row.reads,
        row.workers,
        row.proposals,
    )
}

pub(super) fn work_claim(row: &OwnedWork) -> String {
    if row.partial.is_empty() {
        format!("work {} {} reads {}", row.case, row.class, row.reads)
    } else {
        format!("work {} {} partial {}", row.case, row.class, row.partial)
    }
}

pub(super) fn gap_line(label: &str, row: &OwnedGap) -> String {
    format!(
        "gap {} case {} {} reads {} attempts {} evidence {} mirrors {} workers {} proposals {}",
        label,
        shown(&row.case),
        shown(&row.reason),
        row.reads,
        row.attempts,
        shown(&row.evidence),
        shown(&row.mirrors),
        row.workers,
        row.proposals,
    )
}

fn owned_work(spec: &WorkSpec) -> OwnedWork {
    OwnedWork {
        case: spec.case.to_owned(),
        class: spec.class.to_owned(),
        bytes: spec.bytes.to_owned(),
        files: spec.files.to_owned(),
        tokens: spec.tokens.to_owned(),
        path: spec.path.to_owned(),
        role: spec.role.to_owned(),
        status: spec.status.to_owned(),
        receiver: spec.receiver.to_owned(),
        packet: spec.packet.to_owned(),
        proposal: spec.proposal.to_owned(),
        partial: spec.partial.to_owned(),
        attempts: spec.attempts.to_owned(),
        reads: spec.reads.to_owned(),
        workers: spec.workers.to_owned(),
        proposals: spec.proposals.to_owned(),
    }
}

fn owned_gap(spec: &GapSpec) -> OwnedGap {
    OwnedGap {
        case: spec.case.to_owned(),
        reason: spec.reason.to_owned(),
        reads: spec.reads.to_owned(),
        attempts: spec.attempts.to_owned(),
        evidence: spec.evidence.to_owned(),
        mirrors: spec.mirrors.to_owned(),
        workers: spec.workers.to_owned(),
        proposals: spec.proposals.to_owned(),
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
