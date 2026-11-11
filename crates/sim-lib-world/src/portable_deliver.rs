// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delivery gate for the portable phase union.
//!
//! The leaf calls the loaded deliver command. It qualifies the model,
//! native, and real realizations, then reads model status and why through
//! the qualification's own view. A completed fact stays omitted. Divergence
//! blocks completion. An uncertain cause does not repeat an effect. A private
//! value is refused and is not copied into the deck.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const PLAN: &str = "world/deliver-v1";
const STATUS: &str = "world/deliver-status-v1";
const WHY: &str = "world/deliver-why-v1";
const GAP: &str = "world/deliver-gap-v1";
const ACTION: &str = "world/deliver-action-v1";
const OMISSION: &str = "world/deliver-omission-v1";
const REMAINING: &str = "world/deliver-remaining-v1";

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

#[path = "portable_deliver_expect.rs"]
mod expect;
#[path = "portable_deliver_parse.rs"]
mod parse;

struct Capture {
    model_plan: Datum,
    status: Datum,
    why_done: Datum,
    why_topic: Datum,
    why_sdk: Datum,
    why_private_mirror: Datum,
    why_absent: Datum,
    why_private: Datum,
    plan_private: Datum,
    native_plan: Datum,
    real_plan: Datum,
    stable: bool,
    exports: bool,
}

pub(super) struct OwnedPlan {
    realization: String,
    omitted: Vec<OwnedRow>,
    actions: Vec<OwnedRow>,
    blocked: String,
    noop: String,
    dispatches: String,
    performer_calls: String,
    observations: String,
    effects: String,
    subprocesses: String,
    recovered: String,
    resident: String,
}

#[derive(Clone)]
pub(super) struct OwnedRow {
    realization: String,
    selector: String,
    cause: String,
    class: String,
    acknowledgement: String,
    state: String,
    dispatches: String,
    performer_calls: String,
    observations: String,
    effects: String,
    subprocesses: String,
    desired: String,
    provenance: String,
    value: Option<String>,
}

pub(super) struct OwnedStatus {
    realization: String,
    blocked: String,
    noop: String,
    dispatches: String,
    effects: String,
    performer_calls: String,
    omitted: Vec<String>,
    remaining: Vec<OwnedRemain>,
    subprocesses: String,
    resident: String,
    disclosure: String,
}

pub(super) struct OwnedRemain {
    selector: String,
    cause: String,
    intent: String,
}

pub(super) struct OwnedWhy {
    selector: String,
    cause: String,
    class: String,
    acknowledgement: String,
    dispatches: String,
    effects: String,
    observations: String,
    state: String,
    value: Option<String>,
    resident: String,
}

pub(super) struct OwnedGap {
    realization: String,
    reason: String,
    selector: String,
}

pub(super) fn deliver_leaf() -> Result<ProofLeaf, WorldError> {
    let command = super::project::resident_command()?;
    Ok(leaf_from(capture(&command)?))
}

fn capture(command: &WorldCommandLib) -> Result<Capture, WorldError> {
    let exports = command.manifest().exports.iter().any(|export| {
        matches!(
            export,
            Export::Function { symbol, .. } if symbol.to_string() == ENTRY
        )
    });
    let (model_plan, model_same) = call_pair(command, &plan_argv("model"))?;
    let view = view_text(&model_plan)?;
    let (status, status_same) = call_pair(command, &status_argv("model", &view))?;
    let (why_done, done_same) = call_pair(command, &why_argv("model", "git/branch/main", &view))?;
    let (why_topic, topic_same) =
        call_pair(command, &why_argv("model", "git/branch/topic", &view))?;
    let (why_sdk, sdk_same) = call_pair(command, &why_argv("model", "ci/sim-sdk", &view))?;
    let (why_private_mirror, mirror_same) =
        call_pair(command, &why_argv("model", "mirror/sim-private", &view))?;
    let (why_absent, absent_same) =
        call_pair(command, &why_argv("model", "git/branch/missing", &view))?;
    let (why_private, private_same) =
        call_pair(command, &why_argv("model", "docs/private/note", &view))?;
    let (plan_private, plan_private_same) = call_pair(
        command,
        &one_argv("model", "git/branch/main", "private-material:s3cret-token"),
    )?;
    let (native_plan, native_same) = call_pair(command, &plan_argv("native"))?;
    let (real_plan, real_same) = call_pair(command, &plan_argv("real"))?;
    let stable = model_same
        && status_same
        && done_same
        && topic_same
        && sdk_same
        && mirror_same
        && absent_same
        && private_same
        && plan_private_same
        && native_same
        && real_same;
    Ok(Capture {
        model_plan,
        status,
        why_done,
        why_topic,
        why_sdk,
        why_private_mirror,
        why_absent,
        why_private,
        plan_private,
        native_plan,
        real_plan,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/21".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn plan_argv(realization: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "deliver".to_owned(),
        realization.to_owned(),
    ]
}

fn one_argv(realization: &str, selector: &str, desired: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "deliver".to_owned(),
        realization.to_owned(),
        selector.to_owned(),
        desired.to_owned(),
    ]
}

fn status_argv(realization: &str, view: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "deliver-status".to_owned(),
        realization.to_owned(),
        view.to_owned(),
    ]
}

fn why_argv(realization: &str, selector: &str, view: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "deliver-why".to_owned(),
        realization.to_owned(),
        selector.to_owned(),
        view.to_owned(),
    ]
}

fn call_pair(command: &WorldCommandLib, argv: &[String]) -> Result<(Datum, bool), WorldError> {
    let first = command.run(argv)?;
    let second = command.run(argv)?;
    let same = first == second;
    Ok((first, same))
}

fn view_text(plan: &Datum) -> Result<String, WorldError> {
    match field(plan, "view") {
        Some(Datum::String(text)) if !text.is_empty() => Ok(text.clone()),
        _ => Err(WorldError::Qualification(
            "delivery qualification has no view".to_owned(),
        )),
    }
}

fn actual_deck(capture: &Capture) -> String {
    let values = capture_values(capture);
    let (effects, subprocesses, resident) = meter(capture);
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    push_plan(&mut lines, &capture.model_plan);
    push_status(&mut lines, &capture.status);
    push_why(&mut lines, &capture.why_done);
    push_why(&mut lines, &capture.why_topic);
    push_why(&mut lines, &capture.why_sdk);
    push_why(&mut lines, &capture.why_private_mirror);
    push_gap(&mut lines, "why-absent", &capture.why_absent);
    push_gap(&mut lines, "why-private", &capture.why_private);
    push_gap(&mut lines, "plan-private", &capture.plan_private);
    push_plan(&mut lines, &capture.native_plan);
    push_plan(&mut lines, &capture.real_plan);
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    if values.iter().any(|value| contains_private(value)) {
        lines.push("leaked".to_owned());
    }
    lines.join("\n")
}

fn push_plan(lines: &mut Vec<String>, value: &Datum) {
    let Some(plan) = parse::parsed_plan(value) else {
        lines.push("plan missing".to_owned());
        return;
    };
    lines.push(expect::plan_line_owned(&plan));
    for row in plan.omitted.iter().chain(plan.actions.iter()) {
        let mut owned = row.clone();
        owned.realization = plan.realization.clone();
        lines.push(expect::row_line_owned(&owned));
    }
}

fn push_status(lines: &mut Vec<String>, value: &Datum) {
    let Some(status) = parse::parsed_status(value) else {
        lines.push("status missing".to_owned());
        return;
    };
    lines.push(expect::status_line_owned(&status));
    for row in &status.remaining {
        lines.push(expect::remain_line_owned(row));
    }
}

fn push_why(lines: &mut Vec<String>, value: &Datum) {
    let Some(row) = parse::parsed_why(value) else {
        lines.push("why missing".to_owned());
        return;
    };
    lines.push(expect::why_line_owned(&row));
}

fn push_gap(lines: &mut Vec<String>, label: &str, value: &Datum) {
    let Some(row) = parse::parsed_gap(value) else {
        lines.push(format!("gap {label} missing"));
        return;
    };
    lines.push(expect::gap_line(
        label,
        &row.realization,
        &row.reason,
        &row.selector,
    ));
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(plan) = parse::parsed_plan(&capture.model_plan) {
        for row in &plan.omitted {
            claims.insert(format!("omit {} {}", row.selector, row.cause));
        }
        for row in &plan.actions {
            claims.insert(format!("action {} {}", row.selector, row.cause));
        }
    }
    for (label, value) in [
        ("native", &capture.native_plan),
        ("real", &capture.real_plan),
    ] {
        if let Some(plan) = parse::parsed_plan(value) {
            for row in &plan.actions {
                claims.insert(format!("{label} {} {}", row.selector, row.cause));
            }
        }
    }
    for value in [
        &capture.why_done,
        &capture.why_topic,
        &capture.why_sdk,
        &capture.why_private_mirror,
    ] {
        if let Some(row) = parse::parsed_why(value) {
            claims.insert(format!("why {} {}", row.selector, row.cause));
        }
    }
    if let Some(row) = parse::parsed_gap(&capture.why_absent) {
        claims.insert(format!("gap {} {}", row.selector, row.reason));
    }
    if let (Some(why), Some(_)) = (
        parse::parsed_gap(&capture.why_private),
        parse::parsed_gap(&capture.plan_private),
    ) {
        claims.insert(format!("gap private {}", why.reason));
    }
    claims
}

fn capture_values(capture: &Capture) -> Vec<&Datum> {
    vec![
        &capture.model_plan,
        &capture.status,
        &capture.why_done,
        &capture.why_topic,
        &capture.why_sdk,
        &capture.why_private_mirror,
        &capture.why_absent,
        &capture.why_private,
        &capture.plan_private,
        &capture.native_plan,
        &capture.real_plan,
    ]
}

fn meter(capture: &Capture) -> (String, String, String) {
    let mut effects = "0".to_owned();
    let mut subprocesses = "0".to_owned();
    let mut resident = "true".to_owned();
    for value in [
        &capture.model_plan,
        &capture.native_plan,
        &capture.real_plan,
        &capture.plan_private,
        &capture.why_private,
        &capture.why_absent,
    ] {
        let effect = field_count(value, "effects");
        if effect != "0" {
            effects = effect;
        }
        let subprocess = field_count(value, "subprocesses");
        if subprocess != "0" {
            subprocesses = subprocess;
        }
        let live = field_bool(value, "resident");
        if live != "true" {
            resident = live;
        }
    }
    (effects, subprocesses, resident)
}

fn private(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn contains_private(value: &Datum) -> bool {
    match value {
        Datum::String(text) => private(text),
        Datum::Vector(items) => items.iter().any(contains_private),
        Datum::Node { fields, .. } => fields.iter().any(|(_, item)| contains_private(item)),
        _ => false,
    }
}

fn kind_of(value: &Datum) -> String {
    match value {
        Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}

fn optional_text(value: &Datum, name: &str) -> Option<Option<String>> {
    match field(value, name) {
        None => Some(None),
        Some(Datum::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

fn field_count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Number(number)) if !number.canonical.is_empty() => number.canonical.clone(),
        _ => "missing".to_owned(),
    }
}

fn field_bool(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Bool(true)) => "true".to_owned(),
        Some(Datum::Bool(false)) => "false".to_owned(),
        _ => "missing".to_owned(),
    }
}

fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, field)| field)
}

#[cfg(test)]
#[path = "portable_deliver_tests.rs"]
mod tests;
