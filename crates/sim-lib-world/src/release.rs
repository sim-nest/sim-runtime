// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inspectable release plans over service reconciliation.
//!
//! Each remaining action copies the service plan's class and the original
//! reading. A completed fact is omitted. Status and why read that plan.
//! A private value is refused before it can appear in the plan.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::product::DISCLOSURE_FACT;
use crate::query;
use crate::service;

const REASON_COMPLETED: &str = "completed";
const REASON_DIFFERS: &str = "desired-differs";
const REASON_PRESERVED: &str = "original-preserved";

const PRIVATE_MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Plans the sealed service catalog as a release.
///
/// # Errors
/// Returns an error when the service catalog is not a plan.
pub(crate) fn plan_catalog(realization: &str) -> Result<Datum, WorldError> {
    if let Some(gap) = realization_gap(realization) {
        return Ok(gap);
    }
    project(&service::plan_catalog(realization)?)
}

/// Plans one desired value as a release.
///
/// # Errors
/// Returns an error when the service plan is not a plan.
pub(crate) fn plan_one(
    realization: &str,
    selector: &str,
    desired: &str,
) -> Result<Datum, WorldError> {
    plan(realization, &[(selector, desired)])
}

/// Plans desired values as a release.
///
/// Already completed facts are omitted. Uncertain originals stay on the
/// remaining actions. This function does not perform a release.
///
/// # Errors
/// Returns an error when a request repeats a selector or the service plan
/// is not a plan.
pub(crate) fn plan(realization: &str, requests: &[(&str, &str)]) -> Result<Datum, WorldError> {
    if let Some(gap) = request_gap(realization, requests) {
        return Ok(gap);
    }
    project(&service::plan(realization, requests)?)
}

/// Summarizes the sealed release plan.
///
/// # Errors
/// Returns an error when the sealed plan is not a plan.
pub(crate) fn status(realization: &str) -> Result<Datum, WorldError> {
    let plan = plan_catalog(realization)?;
    if release_gap(&plan).is_some() {
        return Ok(plan);
    }
    let fields = node_fields(&plan, "world/release-plan-v1")?;
    let actions = vector_field(fields, "actions")?;
    let mut remaining = Vec::with_capacity(actions.len());
    for action in &actions {
        let row = node_fields(action, "world/release-action-v1")?;
        remaining.push(remain_datum(
            &required(row, "selector")?,
            &required(row, "reason")?,
            &required(row, "intent")?,
        ));
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "release-status-v1"),
        fields: vec![
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("noop"), Datum::Bool(remaining.is_empty())),
            (Symbol::new("omitted"), field_datum(fields, "omitted")?),
            (Symbol::new("realization"), string(realization)),
            (Symbol::new("remaining"), Datum::Vector(remaining)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    })
}

/// Explains one selector from the sealed release plan.
///
/// # Errors
/// Returns an error when the sealed plan is not a plan.
pub(crate) fn why(realization: &str, selector: &str) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap(realization, selector, "empty-selector"));
    }
    if leaked(selector) {
        return Ok(gap(realization, "", "private-leakage"));
    }
    let plan = plan_catalog(realization)?;
    if release_gap(&plan).is_some() {
        return Ok(plan);
    }
    let fields = node_fields(&plan, "world/release-plan-v1")?;
    for action in vector_field(fields, "actions")? {
        let row = node_fields(&action, "world/release-action-v1")?;
        if required(row, "selector")? == selector {
            return why_datum(realization, row, optional(row, "value")?);
        }
    }
    for omission in vector_field(fields, "omitted")? {
        let row = node_fields(&omission, "world/release-omission-v1")?;
        if required(row, "selector")? == selector {
            return why_datum(realization, row, None);
        }
    }
    Ok(gap(realization, selector, "selector-absent"))
}

fn project(service_plan: &Datum) -> Result<Datum, WorldError> {
    if let Some(fields) = service_gap(service_plan) {
        return Ok(gap(
            &required(fields, "realization")?,
            &required(fields, "selector")?,
            &required(fields, "reason")?,
        ));
    }
    let fields = node_fields(service_plan, "world/service-plan-v1")?;
    let realization = required(fields, "realization")?;
    let mut actions = Vec::new();
    let mut omitted = Vec::new();
    let mut capabilities = BTreeSet::new();
    for selector in strings(field_datum(fields, "omitted")?)? {
        capabilities.insert(policy_of(&realization, &selector)?);
        omitted.push(omission_datum(&selector));
    }
    for fact in vector_field(fields, "retained")? {
        let row = node_fields(&fact, "world/service-fact-v1")?;
        let selector = required(row, "selector")?;
        let desired = required(row, "desired")?;
        let value = optional(row, "value")?;
        if leaked(&selector) || leaked(&desired) || value.as_ref().is_some_and(|item| leaked(item))
        {
            return Ok(gap(&realization, "", "private-leakage"));
        }
        capabilities.insert(policy_of(&realization, &selector)?);
        actions.push(action_datum(row, &selector, &desired, value)?);
    }
    let noop = actions.is_empty();
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "release-plan-v1"),
        fields: vec![
            (Symbol::new("actions"), Datum::Vector(actions)),
            (
                Symbol::new("capabilities"),
                string_vector(capabilities.into_iter().collect()),
            ),
            (Symbol::new("dependencies"), field_datum(fields, "ports")?),
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("noop"), Datum::Bool(noop)),
            (Symbol::new("omitted"), Datum::Vector(omitted)),
            (Symbol::new("realization"), string(&realization)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    })
}

fn realization_gap(realization: &str) -> Option<Datum> {
    leaked(realization).then(|| gap("", "", "private-leakage"))
}

fn request_gap(realization: &str, requests: &[(&str, &str)]) -> Option<Datum> {
    if let Some(gap) = realization_gap(realization) {
        return Some(gap);
    }
    if requests.iter().any(|(selector, _)| selector.is_empty()) {
        return Some(gap(realization, "", "empty-selector"));
    }
    if requests
        .iter()
        .any(|(selector, desired)| leaked(selector) || leaked(desired))
    {
        return Some(gap(realization, "", "private-leakage"));
    }
    None
}

fn action_datum(
    row: &[(Symbol, Datum)],
    selector: &str,
    desired: &str,
    value: Option<String>,
) -> Result<Datum, WorldError> {
    let class = required(row, "class")?;
    let reason = match class.as_str() {
        "divergent" => REASON_DIFFERS,
        "unknown" => REASON_PRESERVED,
        other => {
            return Err(WorldError::Qualification(format!(
                "unknown release class {other}"
            )));
        }
    };
    let mut fields = vec![
        (Symbol::new("class"), string(&class)),
        (Symbol::new("desired"), string(desired)),
        (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
        (Symbol::new("intent"), string(&intent(selector))),
        (Symbol::new("port"), string(&required(row, "port")?)),
        (
            Symbol::new("provenance"),
            string(&required(row, "provenance")?),
        ),
        (Symbol::new("reason"), string(reason)),
        (Symbol::new("selector"), string(selector)),
        (Symbol::new("state"), string(&required(row, "state")?)),
    ];
    if let Some(value) = value {
        fields.push((Symbol::new("value"), string(&value)));
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "release-action-v1"),
        fields,
    })
}

fn omission_datum(selector: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "release-omission-v1"),
        fields: vec![
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (Symbol::new("intent"), string(&intent(selector))),
            (Symbol::new("reason"), string(REASON_COMPLETED)),
            (Symbol::new("selector"), string(selector)),
        ],
    }
}

fn remain_datum(selector: &str, reason: &str, intent: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "release-remaining-v1"),
        fields: vec![
            (Symbol::new("intent"), string(intent)),
            (Symbol::new("reason"), string(reason)),
            (Symbol::new("selector"), string(selector)),
        ],
    }
}

fn why_datum(
    realization: &str,
    row: &[(Symbol, Datum)],
    value: Option<String>,
) -> Result<Datum, WorldError> {
    let mut fields = vec![
        (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
        (Symbol::new("effects"), query::count_datum(0)),
        (Symbol::new("intent"), string(&required(row, "intent")?)),
        (Symbol::new("realization"), string(realization)),
        (Symbol::new("reason"), string(&required(row, "reason")?)),
        (Symbol::new("resident"), Datum::Bool(true)),
        (Symbol::new("selector"), string(&required(row, "selector")?)),
        (Symbol::new("subprocesses"), query::count_datum(0)),
    ];
    if let Ok(class) = required(row, "class") {
        fields.push((Symbol::new("class"), string(&class)));
    }
    if let Ok(state) = required(row, "state") {
        fields.push((Symbol::new("state"), string(&state)));
    }
    if let Some(value) = value {
        fields.push((Symbol::new("value"), string(&value)));
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "release-why-v1"),
        fields,
    })
}

fn policy_of(realization: &str, selector: &str) -> Result<String, WorldError> {
    let snapshot = service::snapshot(realization, selector)?;
    if service_gap(&snapshot).is_some() || release_gap(&snapshot).is_some() {
        return Err(WorldError::Qualification(format!(
            "release capability for {selector} is a gap"
        )));
    }
    let fields = node_fields(&snapshot, "world/service-snapshot-v1")?;
    let observations = vector_field(fields, "observations")?;
    let Some(row) = observations.first() else {
        return Err(WorldError::Qualification(
            "service snapshot has no observation".to_owned(),
        ));
    };
    required(node_fields(row, "world/service-observation-v1")?, "policy")
}

fn intent(selector: &str) -> String {
    format!("intent/reconcile/{selector}")
}

fn leaked(value: &str) -> bool {
    PRIVATE_MARKERS.iter().any(|marker| value.contains(marker))
}

fn gap(realization: &str, selector: &str, reason: &str) -> Datum {
    let realization = if leaked(realization) { "" } else { realization };
    let selector = if leaked(selector) { "" } else { selector };
    let reason = if leaked(reason) {
        "private-leakage"
    } else {
        reason
    };
    Datum::Node {
        tag: Symbol::qualified("world", "release-gap-v1"),
        fields: vec![
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("realization"), string(realization)),
            (Symbol::new("reason"), string(reason)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("selector"), string(selector)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn service_gap(value: &Datum) -> Option<&[(Symbol, Datum)]> {
    match value {
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/service-gap-v1" => {
            Some(fields)
        }
        _ => None,
    }
}

fn release_gap(value: &Datum) -> Option<&[(Symbol, Datum)]> {
    match value {
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/release-gap-v1" => {
            Some(fields)
        }
        _ => None,
    }
}

fn node_fields<'a>(value: &'a Datum, tag: &str) -> Result<&'a [(Symbol, Datum)], WorldError> {
    match value {
        Datum::Node {
            tag: actual,
            fields,
        } if actual.as_qualified_str() == tag => Ok(fields),
        _ => Err(WorldError::Qualification(format!(
            "expected {tag} in a release plan"
        ))),
    }
}

fn vector_field(fields: &[(Symbol, Datum)], name: &str) -> Result<Vec<Datum>, WorldError> {
    match field_datum(fields, name)? {
        Datum::Vector(items) => Ok(items),
        _ => Err(WorldError::Qualification(format!(
            "release field {name} is not a vector"
        ))),
    }
}

fn required(fields: &[(Symbol, Datum)], name: &str) -> Result<String, WorldError> {
    match field_datum(fields, name)? {
        Datum::String(value) => Ok(value),
        _ => Err(WorldError::Qualification(format!(
            "release field {name} is not text"
        ))),
    }
}

fn optional(fields: &[(Symbol, Datum)], name: &str) -> Result<Option<String>, WorldError> {
    match fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
    {
        Some(Datum::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(WorldError::Qualification(format!(
            "release field {name} is not text"
        ))),
        None => Ok(None),
    }
}

fn field_datum(fields: &[(Symbol, Datum)], name: &str) -> Result<Datum, WorldError> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| WorldError::Qualification(format!("release field {name} is missing")))
}

fn strings(value: Datum) -> Result<Vec<String>, WorldError> {
    match value {
        Datum::Vector(items) => items
            .into_iter()
            .map(|item| match item {
                Datum::String(value) => Ok(value),
                _ => Err(WorldError::Qualification(
                    "release text vector contains a non-text item".to_owned(),
                )),
            })
            .collect(),
        _ => Err(WorldError::Qualification(
            "release field is not a text vector".to_owned(),
        )),
    }
}

fn string(value: &str) -> Datum {
    Datum::String(value.to_owned())
}

fn string_vector(values: Vec<String>) -> Datum {
    Datum::Vector(values.into_iter().map(Datum::String).collect())
}
