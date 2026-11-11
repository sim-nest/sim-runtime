// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Schedule record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedSchedule};

struct ScheduleSpec {
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

const FRONTIER: &str = "same 1 partial a:proof-failed order a,c,b zero b,a cycles a:dependency-cycle,b:dependency-cycle,m:missing-dependency";
const RESUME: &str = "failures 3 open 0 same 1";
const WAIT: &str = "admitted b held-a 0 cpu 1 disk 0 waiting a gpu";
const CONTROL: &str = "queued 8 stopped 0 log stop os 0";
const LIVENESS: &str = "stalled s0 since 0 failures 0 work 1 idle 1 beats 7 closed 1 os 0 wake resource-offer,control,fact-refresh";

const SCHEDULE: &[ScheduleSpec] = &[
    ScheduleSpec {
        case: "frontier",
        detail: FRONTIER,
        invokes: "8",
    },
    ScheduleSpec {
        case: "resume",
        detail: RESUME,
        invokes: "4",
    },
    ScheduleSpec {
        case: "wait",
        detail: WAIT,
        invokes: "2",
    },
    ScheduleSpec {
        case: "control",
        detail: CONTROL,
        invokes: "9",
    },
    ScheduleSpec {
        case: "liveness",
        detail: LIVENESS,
        invokes: "10",
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
    for spec in SCHEDULE {
        lines.push(schedule_line(&owned_schedule(spec)));
    }
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for spec in SCHEDULE {
        claims.insert(schedule_claim(&owned_schedule(spec)));
    }
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn schedule_line(row: &OwnedSchedule) -> String {
    format!(
        "schedule {} {} invokes {}",
        shown(&row.case),
        shown(&row.detail),
        row.invokes,
    )
}

pub(super) fn schedule_claim(row: &OwnedSchedule) -> String {
    format!("schedule {} {}", row.case, row.detail)
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

fn owned_schedule(spec: &ScheduleSpec) -> OwnedSchedule {
    OwnedSchedule {
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
