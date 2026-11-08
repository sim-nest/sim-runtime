// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delivery qualification of a release plan against the bound model service.
//!
//! Each row runs the durable operation lifecycle once, cuts the outcome
//! admission, and recovers on the same journal. Recovery observes before any
//! further performance. A completed fact omits performance. A divergent value
//! or a wrong tag blocks completion. A lagging registry, stale observation,
//! denied reading, partial result, uncertain reading, or unavailable receiver
//! stays uncertain. The model sink publishes nothing.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::product::DISCLOSURE_FACT;
use crate::query;
use crate::release;
use crate::service;

#[path = "deliver_run.rs"]
mod run;
#[path = "deliver_view.rs"]
mod view;

const PRIVATE_MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Qualifies the sealed release catalog against the bound model service.
///
/// # Errors
/// Returns an error when a release row or lifecycle result breaks its contract.
pub(crate) fn catalog(realization: &str) -> Result<Datum, WorldError> {
    attach_view(project(&release::plan_catalog(realization)?)?)
}

/// Qualifies one desired value against the bound model service.
///
/// # Errors
/// Returns an error when the release row or lifecycle result breaks its contract.
pub(crate) fn one(realization: &str, selector: &str, desired: &str) -> Result<Datum, WorldError> {
    attach_view(project(&release::plan_one(
        realization,
        selector,
        desired,
    )?)?)
}

/// Summarizes an admitted delivery qualification. It does not qualify again.
///
/// # Errors
/// Returns an error when the admitted record is not a delivery record.
pub(crate) fn status(realization: &str, admitted: Option<&str>) -> Result<Datum, WorldError> {
    let Some(plan) = admitted_record(realization, admitted)? else {
        return Ok(refresh_gap(realization, ""));
    };
    if deliver_gap(&plan).is_some() {
        return Ok(plan);
    }
    let fields = node_fields(&plan, "world/deliver-v1")?;
    let mut remaining = Vec::new();
    for action in vector_field(fields, "actions")? {
        let row = node_fields(&action, "world/deliver-action-v1")?;
        remaining.push(remain_datum(
            &required(row, "selector")?,
            &required(row, "cause")?,
            &required(row, "intent")?,
        ));
    }
    let dispatches = field_datum(fields, "dispatches")?;
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "deliver-status-v1"),
        fields: vec![
            (Symbol::new("blocked"), field_datum(fields, "blocked")?),
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (Symbol::new("dispatches"), dispatches.clone()),
            (Symbol::new("effects"), dispatches),
            (Symbol::new("noop"), field_datum(fields, "noop")?),
            (Symbol::new("omitted"), field_datum(fields, "omitted")?),
            (
                Symbol::new("performer-calls"),
                field_datum(fields, "performer-calls")?,
            ),
            (Symbol::new("realization"), string(realization)),
            (Symbol::new("remaining"), Datum::Vector(remaining)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    })
}

/// Explains one selector from the sealed delivery qualification.
///
/// # Errors
/// Returns an error when the qualification is not a delivery record.
pub(crate) fn why(
    realization: &str,
    selector: &str,
    admitted: Option<&str>,
) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap(realization, selector, "empty-selector"));
    }
    if leaked(selector) || leaked(realization) {
        return Ok(gap(realization, selector, "private-leakage"));
    }
    let Some(plan) = admitted_record(realization, admitted)? else {
        return Ok(refresh_gap(realization, selector));
    };
    if deliver_gap(&plan).is_some() {
        return Ok(plan);
    }
    let fields = node_fields(&plan, "world/deliver-v1")?;
    for action in vector_field(fields, "actions")? {
        let row = node_fields(&action, "world/deliver-action-v1")?;
        if required(row, "selector")? == selector {
            return why_datum(realization, row);
        }
    }
    for omission in vector_field(fields, "omitted")? {
        let row = node_fields(&omission, "world/deliver-omission-v1")?;
        if required(row, "selector")? == selector {
            return why_datum(realization, row);
        }
    }
    Ok(gap(realization, selector, "selector-absent"))
}

fn project(plan: &Datum) -> Result<Datum, WorldError> {
    if let Some(fields) = release_gap(plan) {
        return Ok(gap(
            &required(fields, "realization")?,
            &required(fields, "selector")?,
            &required(fields, "reason")?,
        ));
    }
    let fields = node_fields(plan, "world/release-plan-v1")?;
    let realization = required(fields, "realization")?;
    let mut omitted = Vec::new();
    let mut actions = Vec::new();
    for item in vector_field(fields, "omitted")? {
        let row = node_fields(&item, "world/release-omission-v1")?;
        let selector = required(row, "selector")?;
        match qualify_omission(&realization, row)? {
            Some(qualified) => omitted.push(qualified),
            None => return Ok(gap(&realization, &selector, "private-leakage")),
        }
    }
    for item in vector_field(fields, "actions")? {
        let row = node_fields(&item, "world/release-action-v1")?;
        let selector = required(row, "selector")?;
        match qualify_action(&realization, row)? {
            Some(qualified) => actions.push(qualified),
            None => return Ok(gap(&realization, &selector, "private-leakage")),
        }
    }
    Ok(record_datum(&realization, &omitted, &actions))
}

pub(crate) fn private_input(
    realization: &str,
    selector: &str,
    desired: &str,
    reason: &str,
    value: Option<&str>,
) -> bool {
    leaked(realization)
        || leaked(selector)
        || leaked(desired)
        || leaked(reason)
        || value.is_some_and(leaked)
}

fn qualify_omission(
    realization: &str,
    row: &[(Symbol, Datum)],
) -> Result<Option<run::Qualified>, WorldError> {
    let selector = required(row, "selector")?;
    let reason = required(row, "reason")?;
    let reading = reading(realization, &selector)?;
    if private_input(
        realization,
        &selector,
        reading.value.as_deref().unwrap_or(""),
        &reason,
        reading.value.as_deref(),
    ) {
        return Ok(None);
    }
    if reading.state != "current" {
        return Err(WorldError::Qualification(format!(
            "completed {selector} is not current"
        )));
    }
    let desired = reading
        .value
        .clone()
        .ok_or_else(|| WorldError::Qualification(format!("completed {selector} has no value")))?;
    run::qualify(realization, &selector, &desired, &reason, &reading)
}

fn qualify_action(
    realization: &str,
    row: &[(Symbol, Datum)],
) -> Result<Option<run::Qualified>, WorldError> {
    let selector = required(row, "selector")?;
    let desired = required(row, "desired")?;
    let reason = required(row, "reason")?;
    let reading = reading(realization, &selector)?;
    if private_input(
        realization,
        &selector,
        &desired,
        &reason,
        reading.value.as_deref(),
    ) {
        return Ok(None);
    }
    if reading.state != required(row, "state")? {
        return Err(WorldError::Qualification(format!(
            "delivery state for {selector} disagrees with the release"
        )));
    }
    run::qualify(realization, &selector, &desired, &reason, &reading)
}

pub(super) struct Reading {
    pub(super) kind: String,
    pub(super) provenance: String,
    pub(super) state: String,
    pub(super) value: Option<String>,
}

fn reading(realization: &str, selector: &str) -> Result<Reading, WorldError> {
    let snapshot = service::snapshot(realization, selector)?;
    if let Some(fields) = service_gap(&snapshot) {
        return Err(WorldError::Qualification(format!(
            "delivery snapshot for {selector} is {}",
            required(fields, "reason")?
        )));
    }
    let fields = node_fields(&snapshot, "world/service-snapshot-v1")?;
    let observations = vector_field(fields, "observations")?;
    let Some(observation) = observations.first() else {
        return Err(WorldError::Qualification(format!(
            "delivery snapshot for {selector} has no observation"
        )));
    };
    let row = node_fields(observation, "world/service-observation-v1")?;
    Ok(Reading {
        kind: required(row, "kind")?,
        provenance: required(row, "provenance")?,
        state: required(row, "state")?,
        value: optional(row, "value")?,
    })
}

fn record_datum(
    realization: &str,
    omitted: &[run::Qualified],
    actions: &[run::Qualified],
) -> Datum {
    let rows = omitted.iter().chain(actions.iter());
    let dispatches = rows.clone().map(|row| row.dispatches).sum();
    let performer_calls = rows.clone().map(|row| row.performer_calls).sum();
    let observations = rows.map(|row| row.observations).sum();
    Datum::Node {
        tag: Symbol::qualified("world", "deliver-v1"),
        fields: vec![
            (
                Symbol::new("actions"),
                Datum::Vector(actions.iter().map(action_datum).collect()),
            ),
            (Symbol::new("blocked"), Datum::Bool(!actions.is_empty())),
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (Symbol::new("dispatches"), query::count_datum(dispatches)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("noop"), Datum::Bool(actions.is_empty())),
            (
                Symbol::new("observations"),
                query::count_datum(observations),
            ),
            (
                Symbol::new("omitted"),
                Datum::Vector(omitted.iter().map(omission_datum).collect()),
            ),
            (
                Symbol::new("performer-calls"),
                query::count_datum(performer_calls),
            ),
            (Symbol::new("realization"), string(realization)),
            (Symbol::new("recovered"), Datum::Bool(true)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn action_datum(row: &run::Qualified) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "deliver-action-v1"),
        fields: row_fields(row),
    }
}

fn omission_datum(row: &run::Qualified) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "deliver-omission-v1"),
        fields: row_fields(row),
    }
}

fn row_fields(row: &run::Qualified) -> Vec<(Symbol, Datum)> {
    let mut fields = vec![
        (Symbol::new("acknowledgement"), string(row.acknowledgement)),
        (Symbol::new("cause"), string(row.cause)),
        (Symbol::new("class"), string(row.class)),
        (Symbol::new("desired"), string(&row.desired)),
        (
            Symbol::new("dispatches"),
            query::count_datum(row.dispatches),
        ),
        (Symbol::new("effects"), query::count_datum(0)),
        (Symbol::new("intent"), string(&row.intent)),
        (
            Symbol::new("observations"),
            query::count_datum(row.observations),
        ),
        (
            Symbol::new("performer-calls"),
            query::count_datum(row.performer_calls),
        ),
        (Symbol::new("provenance"), string(&row.provenance)),
        (Symbol::new("selector"), string(&row.selector)),
        (Symbol::new("state"), string(&row.state)),
        (Symbol::new("subprocesses"), query::count_datum(0)),
    ];
    if let Some(value) = &row.value {
        fields.push((Symbol::new("value"), string(value)));
    }
    fields
}

fn remain_datum(selector: &str, cause: &str, intent: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "deliver-remaining-v1"),
        fields: vec![
            (Symbol::new("cause"), string(cause)),
            (Symbol::new("intent"), string(intent)),
            (Symbol::new("selector"), string(selector)),
        ],
    }
}

fn why_datum(realization: &str, row: &[(Symbol, Datum)]) -> Result<Datum, WorldError> {
    let dispatches = field_datum(row, "dispatches")?;
    let mut fields = vec![
        (
            Symbol::new("acknowledgement"),
            string(&required(row, "acknowledgement")?),
        ),
        (Symbol::new("cause"), string(&required(row, "cause")?)),
        (Symbol::new("class"), string(&required(row, "class")?)),
        (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
        (Symbol::new("dispatches"), dispatches.clone()),
        (Symbol::new("effects"), dispatches),
        (Symbol::new("intent"), string(&required(row, "intent")?)),
        (
            Symbol::new("observations"),
            field_datum(row, "observations")?,
        ),
        (
            Symbol::new("performer-calls"),
            field_datum(row, "performer-calls")?,
        ),
        (Symbol::new("realization"), string(realization)),
        (Symbol::new("resident"), Datum::Bool(true)),
        (Symbol::new("selector"), string(&required(row, "selector")?)),
        (Symbol::new("state"), string(&required(row, "state")?)),
        (Symbol::new("subprocesses"), query::count_datum(0)),
    ];
    if let Some(value) = optional(row, "value")? {
        fields.push((Symbol::new("value"), string(&value)));
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "deliver-why-v1"),
        fields,
    })
}

fn attach_view(record: Datum) -> Result<Datum, WorldError> {
    let Datum::Node { tag, mut fields } = record else {
        return Ok(record);
    };
    let bare = Datum::Node {
        tag: tag.clone(),
        fields: fields.clone(),
    };
    fields.push((Symbol::new("view"), string(&view::export(&bare)?)));
    Ok(Datum::Node { tag, fields })
}

fn admitted_record(realization: &str, admitted: Option<&str>) -> Result<Option<Datum>, WorldError> {
    let Some(admitted) = admitted else {
        return Ok(None);
    };
    let Some(record) = view::open(admitted)? else {
        return Ok(None);
    };
    let Some(found) = realization_of(&record) else {
        return Ok(None);
    };
    if found != realization {
        return Ok(None);
    }
    if !query_shape(&record) {
        return Ok(None);
    }
    Ok(Some(record))
}

fn query_shape(record: &Datum) -> bool {
    if let Some(fields) = deliver_gap(record) {
        return gap_ready(fields);
    }
    let Ok(fields) = node_fields(record, "world/deliver-v1") else {
        return false;
    };
    if !bool_field(fields, "blocked")
        || !number_field(fields, "dispatches")
        || !bool_field(fields, "noop")
        || !number_field(fields, "performer-calls")
    {
        return false;
    }
    rows_ready(fields, "actions", "world/deliver-action-v1")
        && rows_ready(fields, "omitted", "world/deliver-omission-v1")
}

fn rows_ready(fields: &[(Symbol, Datum)], name: &str, tag: &str) -> bool {
    let Ok(items) = vector_field(fields, name) else {
        return false;
    };
    items.iter().all(|item| row_ready(item, tag))
}

fn row_ready(item: &Datum, tag: &str) -> bool {
    let Ok(row) = node_fields(item, tag) else {
        return false;
    };
    [
        "acknowledgement",
        "cause",
        "class",
        "intent",
        "selector",
        "state",
    ]
    .iter()
    .all(|name| required(row, name).is_ok())
        && number_field(row, "dispatches")
        && number_field(row, "observations")
        && number_field(row, "performer-calls")
        && value_is_text(row)
}

fn gap_ready(fields: &[(Symbol, Datum)]) -> bool {
    text_field(fields, "realization")
        && text_field(fields, "reason")
        && text_field(fields, "selector")
        && matches!(typed_field(fields, "resident"), Some(Datum::Bool(_)))
        && matches!(typed_field(fields, "effects"), Some(Datum::Number(_)))
        && matches!(typed_field(fields, "subprocesses"), Some(Datum::Number(_)))
}

fn value_is_text(fields: &[(Symbol, Datum)]) -> bool {
    match typed_field(fields, "value") {
        None | Some(Datum::String(_)) => true,
        Some(_) => false,
    }
}

fn text_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(typed_field(fields, name), Some(Datum::String(_)))
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(typed_field(fields, name), Some(Datum::Bool(_)))
}

fn number_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(typed_field(fields, name), Some(Datum::Number(_)))
}

fn typed_field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

fn realization_of(record: &Datum) -> Option<String> {
    let Datum::Node { tag, fields } = record else {
        return None;
    };
    let kind = tag.as_qualified_str();
    if kind != "world/deliver-v1" && kind != "world/deliver-gap-v1" {
        return None;
    }
    fields.iter().find_map(|(name, value)| {
        if name.as_qualified_str() == "realization" {
            match value {
                Datum::String(value) => Some(value.clone()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn refresh_gap(realization: &str, selector: &str) -> Datum {
    let mut record = gap(realization, selector, "refresh-needed");
    let Datum::Node { fields, .. } = &mut record else {
        return record;
    };
    if let Some((_, resident)) = fields
        .iter_mut()
        .find(|(name, _)| name.as_qualified_str() == "resident")
    {
        *resident = Datum::Bool(false);
    }
    record
}

#[cfg(test)]
pub(crate) fn qualify_calls() -> usize {
    run::qualify_calls()
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
        tag: Symbol::qualified("world", "deliver-gap-v1"),
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

pub(super) fn leaked(value: &str) -> bool {
    PRIVATE_MARKERS.iter().any(|marker| value.contains(marker))
}

fn release_gap(value: &Datum) -> Option<&[(Symbol, Datum)]> {
    match value {
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/release-gap-v1" => {
            Some(fields)
        }
        _ => None,
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

fn deliver_gap(value: &Datum) -> Option<&[(Symbol, Datum)]> {
    match value {
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/deliver-gap-v1" => {
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
            "expected {tag} in a delivery qualification"
        ))),
    }
}

fn vector_field(fields: &[(Symbol, Datum)], name: &str) -> Result<Vec<Datum>, WorldError> {
    match field_datum(fields, name)? {
        Datum::Vector(items) => Ok(items),
        _ => Err(WorldError::Qualification(format!(
            "delivery field {name} is not a vector"
        ))),
    }
}

fn required(fields: &[(Symbol, Datum)], name: &str) -> Result<String, WorldError> {
    match field_datum(fields, name)? {
        Datum::String(value) => Ok(value),
        _ => Err(WorldError::Qualification(format!(
            "delivery field {name} is not text"
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
            "delivery field {name} is not text"
        ))),
        None => Ok(None),
    }
}

fn field_datum(fields: &[(Symbol, Datum)], name: &str) -> Result<Datum, WorldError> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| WorldError::Qualification(format!("delivery field {name} is missing")))
}

fn string(value: &str) -> Datum {
    Datum::String(value.to_owned())
}
