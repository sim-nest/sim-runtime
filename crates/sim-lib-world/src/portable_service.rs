// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Service gate for the portable phase union.
//!
//! The leaf calls the loaded service command. It reads each authored model
//! selector, plans that realization, and asks the native and real
//! realizations to read and plan. A current match is omitted. A divergent
//! or unknown fact stays on the plan. Each row keeps the reads and the
//! provenance from that request. A count keeps an empty list distinct from
//! a blank token and from one joined token. The actual deck is that command
//! record. The expected deck is the authored service record.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

const ENTRY: &str = "cli/main/world";
const SNAPSHOT: &str = "world/service-snapshot-v1";
const PLAN: &str = "world/service-plan-v1";

#[path = "portable_service_expect.rs"]
mod expect;

struct Capture {
    rows: Vec<Datum>,
    model_plan: Datum,
    native: Datum,
    real: Datum,
    native_plan: Datum,
    real_plan: Datum,
    stable: bool,
    exports: bool,
}

struct Snapshot {
    realization: String,
    selector: String,
    state: String,
    port: String,
    provenance: String,
    value: Option<String>,
    reads: Vec<String>,
}

struct PlanRow {
    realization: String,
    omitted: Vec<String>,
    actions: Vec<(String, String)>,
    reads: Vec<String>,
    ports: Vec<String>,
    retained: Vec<Fact>,
}

struct Fact {
    selector: String,
    class: String,
    state: String,
    port: String,
    provenance: String,
    value: Option<String>,
    desired: String,
}

pub(super) fn service_leaf() -> Result<ProofLeaf, WorldError> {
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
    let mut rows = Vec::with_capacity(expect::ROWS.len());
    let mut stable = true;
    for reading in expect::ROWS {
        let (value, same) = call_pair(command, &snapshot_argv("model", reading.selector))?;
        stable &= same;
        rows.push(value);
    }
    let (model_plan, model_same) = call_pair(command, &plan_argv("model"))?;
    let (native, native_same) = call_pair(command, &snapshot_argv("native", expect::PROBE))?;
    let (real, real_same) = call_pair(command, &snapshot_argv("real", expect::PROBE))?;
    let (native_plan, native_plan_same) = call_pair(command, &plan_argv("native"))?;
    let (real_plan, real_plan_same) = call_pair(command, &plan_argv("real"))?;
    stable &= model_same && native_same && real_same && native_plan_same && real_plan_same;
    Ok(Capture {
        rows,
        model_plan,
        native,
        real,
        native_plan,
        real_plan,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/19".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn snapshot_argv(realization: &str, selector: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "service".to_owned(),
        realization.to_owned(),
        selector.to_owned(),
    ]
}

fn plan_argv(realization: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "service-plan".to_owned(),
        realization.to_owned(),
    ]
}

fn call_pair(command: &WorldCommandLib, argv: &[String]) -> Result<(Datum, bool), WorldError> {
    let first = command.run(argv)?;
    let second = command.run(argv)?;
    let same = first == second;
    Ok((first, same))
}

fn actual_deck(capture: &Capture) -> String {
    let entry = if capture.exports { ENTRY } else { "missing" };
    let values = capture_values(capture);
    let (effects, subprocesses, resident) = meter(&values);
    let mut lines = vec![
        format!("entry {entry}"),
        format!("kind {}", shared_kind(capture)),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    for (index, reading) in expect::ROWS.iter().enumerate() {
        lines.push(snapshot_line(
            "model",
            reading.selector,
            capture.rows.get(index),
        ));
    }
    push_plan(&mut lines, "model", &capture.model_plan);
    lines.push(snapshot_line(
        "native",
        expect::PROBE,
        Some(&capture.native),
    ));
    lines.push(snapshot_line("real", expect::PROBE, Some(&capture.real)));
    push_plan(&mut lines, "native", &capture.native_plan);
    push_plan(&mut lines, "real", &capture.real_plan);
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn snapshot_line(asked: &str, selector: &str, value: Option<&Datum>) -> String {
    match value.and_then(parsed_snapshot) {
        Some(row) => format!(
            "row {} {} state {} port {} provenance {} value {} reads {}",
            row.realization,
            row.selector,
            row.state,
            counted_str(&row.port),
            counted_str(&row.provenance),
            counted_one(row.value.as_deref()),
            counted(&row.reads)
        ),
        None => format!("row {asked} {selector} missing"),
    }
}

fn push_plan(lines: &mut Vec<String>, asked: &str, value: &Datum) {
    let Some(plan) = parsed_plan(value) else {
        lines.push(format!("plan {asked} missing"));
        return;
    };
    lines.push(format!(
        "plan {} omitted {} actions {} reads {} ports {}",
        plan.realization,
        counted(&plan.omitted),
        parsed_actions(&plan.actions),
        counted(&plan.reads),
        counted(&plan.ports)
    ));
    for fact in &plan.retained {
        lines.push(format!(
            "kept {} {} class {} state {} port {} provenance {} value {} desired {}",
            plan.realization,
            fact.selector,
            fact.class,
            fact.state,
            counted_str(&fact.port),
            counted_str(&fact.provenance),
            counted_one(fact.value.as_deref()),
            counted_str(&fact.desired)
        ));
    }
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for (index, _) in expect::ROWS.iter().enumerate() {
        if let Some(row) = capture.rows.get(index).and_then(parsed_snapshot) {
            claims.insert(format!("state {} {}", row.selector, row.state));
        }
    }
    if let Some(plan) = parsed_plan(&capture.model_plan) {
        for selector in &plan.omitted {
            claims.insert(format!("omitted {selector}"));
        }
        for (selector, class) in &plan.actions {
            claims.insert(format!("action {selector} {class}"));
        }
        for fact in &plan.retained {
            claims.insert(format!("kept {} {}", fact.selector, fact.class));
        }
    }
    if let Some(row) = parsed_snapshot(&capture.native) {
        claims.insert(format!("native {} {}", row.selector, row.state));
    }
    if let Some(row) = parsed_snapshot(&capture.real) {
        claims.insert(format!("real {} {}", row.selector, row.state));
    }
    claims
}

fn capture_values(capture: &Capture) -> Vec<&Datum> {
    let mut values = Vec::new();
    values.extend(capture.rows.iter());
    values.push(&capture.model_plan);
    values.push(&capture.native);
    values.push(&capture.real);
    values.push(&capture.native_plan);
    values.push(&capture.real_plan);
    values
}

fn parsed_snapshot(value: &Datum) -> Option<Snapshot> {
    if kind_of(value) != SNAPSHOT {
        return None;
    }
    let observation = first_child(value, "observations")?;
    let realization = field_text(value, "realization");
    let selector = field_text(value, "selector");
    let state = field_text(&observation, "state");
    let port = field_text(&observation, "port");
    let provenance = field_text(&observation, "provenance");
    if [
        realization.as_str(),
        selector.as_str(),
        state.as_str(),
        port.as_str(),
        provenance.as_str(),
    ]
    .contains(&"missing")
    {
        return None;
    }
    Some(Snapshot {
        realization,
        selector,
        state,
        port,
        provenance,
        value: optional_text(&observation, "value")?,
        reads: string_list(field(value, "reads"))?,
    })
}

fn parsed_plan(value: &Datum) -> Option<PlanRow> {
    if kind_of(value) != PLAN {
        return None;
    }
    let realization = field_text(value, "realization");
    if realization == "missing" {
        return None;
    }
    let actions = action_list(field(value, "actions"))?;
    let retained = fact_list(field(value, "retained"))?;
    Some(PlanRow {
        realization,
        omitted: string_list(field(value, "omitted"))?,
        actions,
        reads: string_list(field(value, "reads"))?,
        ports: string_list(field(value, "ports"))?,
        retained,
    })
}

fn action_list(value: Option<&Datum>) -> Option<Vec<(String, String)>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            let selector = field_text(item, "selector");
            let class = field_text(item, "class");
            if selector == "missing" || class == "missing" {
                None
            } else {
                Some((selector, class))
            }
        })
        .collect()
}

fn fact_list(value: Option<&Datum>) -> Option<Vec<Fact>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            let selector = field_text(item, "selector");
            let class = field_text(item, "class");
            let state = field_text(item, "state");
            let port = field_text(item, "port");
            let provenance = field_text(item, "provenance");
            let desired = field_text(item, "desired");
            if [
                selector.as_str(),
                class.as_str(),
                state.as_str(),
                port.as_str(),
                provenance.as_str(),
                desired.as_str(),
            ]
            .contains(&"missing")
            {
                return None;
            }
            Some(Fact {
                selector,
                class,
                state,
                port,
                provenance,
                value: optional_text(item, "value")?,
                desired,
            })
        })
        .collect()
}

fn first_child(value: &Datum, name: &str) -> Option<Datum> {
    let Datum::Vector(items) = field(value, name)? else {
        return None;
    };
    items.first().cloned()
}

fn shared_kind(capture: &Capture) -> String {
    let mut kinds = capture.rows.iter().map(kind_of).collect::<BTreeSet<_>>();
    kinds.insert(kind_of(&capture.native));
    kinds.insert(kind_of(&capture.real));
    if kinds.len() == 1 {
        kinds
            .into_iter()
            .next()
            .unwrap_or_else(|| "missing".to_owned())
    } else {
        "mixed".to_owned()
    }
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

fn counted(values: &[String]) -> String {
    if values.is_empty() {
        "0".to_owned()
    } else {
        format!("{}|{}", values.len(), values.join("|"))
    }
}

fn counted_str(value: &str) -> String {
    format!("1|{value}")
}

fn counted_one(value: Option<&str>) -> String {
    match value {
        Some(text) => counted_str(text),
        None => "0".to_owned(),
    }
}

fn parsed_actions(actions: &[(String, String)]) -> String {
    if actions.is_empty() {
        "0".to_owned()
    } else {
        let parts = actions
            .iter()
            .map(|(selector, class)| format!("{selector}:{class}"))
            .collect::<Vec<_>>();
        format!("{}|{}", parts.len(), parts.join("|"))
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
#[path = "portable_service_tests.rs"]
mod tests;
