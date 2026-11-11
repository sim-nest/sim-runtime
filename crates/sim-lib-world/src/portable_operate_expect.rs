// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Operation record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedGap, OwnedOperate};

struct OperateSpec {
    case: &'static str,
    verified: &'static str,
    replay: &'static str,
    uncertain: &'static str,
    forged: &'static str,
    interrupts: &'static str,
    recovered: &'static str,
    counter: &'static str,
    invokes: &'static str,
}

struct GapSpec {
    label: &'static str,
    case: &'static str,
    reason: &'static str,
    evidence: &'static str,
}

/// One verification, one replay, one missing acknowledgement, one reread,
/// and fourteen interruptions each recovered once.
const INVOKES: &str = "32";

/// Before and after each of the seven durable admissions.
const INTERRUPTS: &str = "14";

const MEASURE: OperateSpec = OperateSpec {
    case: "measure",
    verified: "verified",
    replay: "held",
    uncertain: "uncertain",
    forged: "refused",
    interrupts: INTERRUPTS,
    recovered: INTERRUPTS,
    counter: "same",
    invokes: INVOKES,
};

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
    lines.push(operate_line(&owned_operate(&MEASURE)));
    for gap in GAPS {
        lines.push(gap_line(gap.label, &owned_gap(gap)));
    }
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    claims.insert(operate_claim(&owned_operate(&MEASURE)));
    for spec in GAPS {
        claims.insert(format!("gap {} {}", spec.label, spec.reason));
    }
    claims
}

pub(super) fn operate_line(row: &OwnedOperate) -> String {
    format!(
        "operate {} {} {} {} {} interrupts {} recovered {} counter {} invokes {}",
        shown(&row.case),
        shown(&row.verified),
        shown(&row.replay),
        shown(&row.uncertain),
        shown(&row.forged),
        row.interrupts,
        row.recovered,
        shown(&row.counter),
        row.invokes,
    )
}

pub(super) fn operate_claim(row: &OwnedOperate) -> String {
    format!(
        "operate {} {} {} {} {}",
        row.case, row.verified, row.replay, row.uncertain, row.forged
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

fn owned_operate(spec: &OperateSpec) -> OwnedOperate {
    OwnedOperate {
        case: spec.case.to_owned(),
        verified: spec.verified.to_owned(),
        replay: spec.replay.to_owned(),
        uncertain: spec.uncertain.to_owned(),
        forged: spec.forged.to_owned(),
        interrupts: spec.interrupts.to_owned(),
        recovered: spec.recovered.to_owned(),
        counter: spec.counter.to_owned(),
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
    value.to_owned()
}
