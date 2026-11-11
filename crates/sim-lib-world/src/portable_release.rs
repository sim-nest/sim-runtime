// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Release gate for the portable phase union.
//!
//! The leaf calls the loaded release command. It plans the model, native,
//! and real realizations, then reads model status and why. A completed fact
//! stays omitted. A divergent or uncertain fact stays, with its original
//! value. A private value is refused and is not copied into the deck.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const PLAN: &str = "world/release-plan-v1";
const STATUS: &str = "world/release-status-v1";
const WHY: &str = "world/release-why-v1";
const GAP: &str = "world/release-gap-v1";
const ACTION: &str = "world/release-action-v1";
const OMISSION: &str = "world/release-omission-v1";
const REMAINING: &str = "world/release-remaining-v1";

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

#[path = "portable_release_expect.rs"]
mod expect;
#[path = "portable_release_parse.rs"]
mod parse;

struct Capture {
    model_plan: Datum,
    status: Datum,
    why_done: Datum,
    why_topic: Datum,
    why_sdk: Datum,
    why_absent: Datum,
    why_private: Datum,
    plan_private: Datum,
    native_plan: Datum,
    real_plan: Datum,
    stable: bool,
    exports: bool,
}

struct PlanRow {
    realization: String,
    omitted: Vec<String>,
    actions: Vec<Act>,
    dependencies: Vec<String>,
    capabilities: Vec<String>,
    noop: String,
    disclosure: String,
}

struct Act {
    selector: String,
    class: String,
    reason: String,
    state: String,
    port: String,
    provenance: String,
    value: Option<String>,
    desired: String,
    intent: String,
    disclosure: String,
}

struct WhyRow {
    selector: String,
    reason: String,
    class: Option<String>,
    state: Option<String>,
    value: Option<String>,
    intent: String,
}

struct GapRow {
    realization: String,
    reason: String,
    selector: String,
}

struct Remain {
    selector: String,
    reason: String,
    intent: String,
}

struct StatusRow {
    realization: String,
    omitted: Vec<String>,
    noop: String,
    disclosure: String,
    remaining: Vec<Remain>,
}

pub(super) fn release_leaf() -> Result<ProofLeaf, WorldError> {
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
    let (status, status_same) = call_pair(command, &status_argv("model"))?;
    let (why_done, done_same) = call_pair(command, &why_argv("model", "git/branch/main"))?;
    let (why_topic, topic_same) = call_pair(command, &why_argv("model", "git/branch/topic"))?;
    let (why_sdk, sdk_same) = call_pair(command, &why_argv("model", "ci/sim-sdk"))?;
    let (why_absent, absent_same) = call_pair(command, &why_argv("model", "git/branch/missing"))?;
    let (why_private, private_same) = call_pair(command, &why_argv("model", "docs/private/note"))?;
    let (plan_private, plan_private_same) = call_pair(
        command,
        &plan_one_argv("model", "git/branch/main", "private-material:s3cret-token"),
    )?;
    let (native_plan, native_same) = call_pair(command, &plan_argv("native"))?;
    let (real_plan, real_same) = call_pair(command, &plan_argv("real"))?;
    let stable = model_same
        && status_same
        && done_same
        && topic_same
        && sdk_same
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
        name: "phase/20".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn plan_argv(realization: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "release-plan".to_owned(),
        realization.to_owned(),
    ]
}

fn plan_one_argv(realization: &str, selector: &str, desired: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "release-plan".to_owned(),
        realization.to_owned(),
        selector.to_owned(),
        desired.to_owned(),
    ]
}

fn status_argv(realization: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "release-status".to_owned(),
        realization.to_owned(),
    ]
}

fn why_argv(realization: &str, selector: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "release-why".to_owned(),
        realization.to_owned(),
        selector.to_owned(),
    ]
}

fn call_pair(command: &WorldCommandLib, argv: &[String]) -> Result<(Datum, bool), WorldError> {
    let first = command.run(argv)?;
    let second = command.run(argv)?;
    let same = first == second;
    Ok((first, same))
}

fn actual_deck(capture: &Capture) -> String {
    let values = capture_values(capture);
    let (effects, subprocesses, resident) = meter(&values);
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    push_plan(&mut lines, "model", &capture.model_plan);
    push_status(&mut lines, &capture.status);
    push_why(&mut lines, "git/branch/main", &capture.why_done);
    push_why(&mut lines, "git/branch/topic", &capture.why_topic);
    push_why(&mut lines, "ci/sim-sdk", &capture.why_sdk);
    push_gap(&mut lines, "why-absent", &capture.why_absent);
    push_gap(&mut lines, "why-private", &capture.why_private);
    push_gap(&mut lines, "plan-private", &capture.plan_private);
    push_plan(&mut lines, "native", &capture.native_plan);
    push_plan(&mut lines, "real", &capture.real_plan);
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    if values.iter().any(|value| contains_private(value)) {
        lines.push("leaked".to_owned());
    }
    lines.join("\n")
}

fn push_plan(lines: &mut Vec<String>, asked: &str, value: &Datum) {
    let Some(plan) = parse::parsed_plan(value) else {
        lines.push(format!("plan {asked} missing"));
        return;
    };
    let pairs = plan
        .actions
        .iter()
        .map(|action| (action.selector.clone(), action.reason.clone()))
        .collect::<Vec<_>>();
    lines.push(expect::plan_line(
        &plan.realization,
        &plan.omitted,
        &pairs,
        &plan.dependencies,
        &plan.capabilities,
        plan.noop == "true",
        &plan.disclosure,
    ));
    for selector in &plan.omitted {
        let Some(omission) = parse::omission_of(value, selector) else {
            lines.push(format!("omit {asked} {selector} missing"));
            continue;
        };
        lines.push(expect::omit_line(
            &plan.realization,
            selector,
            &omission.0,
            &omission.1,
            &omission.2,
        ));
    }
    for action in &plan.actions {
        lines.push(expect::act_line(&expect::ActLine {
            realization: &plan.realization,
            selector: &action.selector,
            class: &action.class,
            reason: &action.reason,
            state: &action.state,
            port: &action.port,
            provenance: &action.provenance,
            value: action.value.as_deref(),
            desired: &action.desired,
            intent: &action.intent,
            disclosure: &action.disclosure,
        }));
    }
}

fn push_status(lines: &mut Vec<String>, value: &Datum) {
    let Some(plan) = parse::parsed_status(value) else {
        lines.push("status model missing".to_owned());
        return;
    };
    let pairs = plan
        .remaining
        .iter()
        .map(|row| (row.selector.clone(), row.reason.clone()))
        .collect::<Vec<_>>();
    lines.push(expect::status_line(
        &plan.realization,
        &pairs,
        &plan.omitted,
        plan.noop == "true",
        &plan.disclosure,
    ));
    for row in &plan.remaining {
        lines.push(expect::remain_line(&row.selector, &row.reason, &row.intent));
    }
}

fn push_why(lines: &mut Vec<String>, asked: &str, value: &Datum) {
    let Some(row) = parse::parsed_why(value) else {
        lines.push(format!("why {asked} missing"));
        return;
    };
    lines.push(expect::why_line(
        &row.selector,
        &row.reason,
        row.class.as_deref(),
        row.state.as_deref(),
        row.value.as_deref(),
        &row.intent,
    ));
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
        for selector in &plan.omitted {
            claims.insert(format!("omit {selector}"));
        }
        for action in &plan.actions {
            claims.insert(format!("action {} {}", action.selector, action.reason));
        }
    }
    for (label, value) in [
        ("native", &capture.native_plan),
        ("real", &capture.real_plan),
    ] {
        if let Some(plan) = parse::parsed_plan(value) {
            for action in &plan.actions {
                claims.insert(format!("{label} {} {}", action.selector, action.reason));
            }
        }
    }
    for value in [&capture.why_done, &capture.why_topic, &capture.why_sdk] {
        if let Some(row) = parse::parsed_why(value) {
            claims.insert(format!("why {} {}", row.selector, row.reason));
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
        &capture.why_absent,
        &capture.why_private,
        &capture.plan_private,
        &capture.native_plan,
        &capture.real_plan,
    ]
}

fn meter(values: &[&Datum]) -> (String, String, String) {
    let mut effects = "0".to_owned();
    let mut subprocesses = "0".to_owned();
    let mut resident = "true".to_owned();
    for value in values {
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

fn string_list(value: Option<&Datum>) -> Option<Vec<String>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            Datum::String(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
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

fn field_text(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::String(text)) => text.clone(),
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
#[path = "portable_release_tests.rs"]
mod tests;
