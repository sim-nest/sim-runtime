// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit model readings of external service facts.
//!
//! Branch, tag, direct, and peeled targets, package checksums, and CI and
//! mirror facts come from the named port. Stale, partial, denied, lagging,
//! and uncertain stay distinct from a current value. A desired value is a
//! caller input to the plan. It is not an observation field. A current match
//! is omitted. A divergent or unknown fact is retained. Observation and
//! planning do not mutate the readings. A native or real-service request does
//! not consult the model readings. Provenance stays on the reading and remains
//! after its byte encoding is decoded.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

const POLICY_MODEL: &str = "policy/explicit-model-v1";
const POLICY_NATIVE: &str = "policy/no-native-fallback-v1";
const POLICY_REAL: &str = "policy/no-real-service-fallback-v1";
const PROVENANCE_NATIVE: &str = "native-capability-missing";
const PROVENANCE_REAL: &str = "real-service-unbound";

#[path = "service_facts.rs"]
mod service_facts;
#[path = "service_wire.rs"]
mod service_wire;

#[cfg(test)]
pub(crate) use service_wire::{decode_observation, encode_observation};

struct Reading {
    kind: String,
    policy: &'static str,
    port: String,
    provenance: String,
    selector: String,
    state: String,
    value: Option<String>,
    called: bool,
}

/// Reads one external-service selector from the named realization.
///
/// # Errors
/// Returns an error when a sealed row breaks the state and value binding.
pub(crate) fn snapshot(realization: &str, selector: &str) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap(realization, selector, "empty-selector"));
    }
    let Some(mode) = mode_of(realization) else {
        return Ok(gap(realization, selector, "unknown-realization"));
    };
    let reading = read_one(mode, selector)?;
    let value = snapshot_datum(realization, selector, &reading);
    service_wire::preserve(&value)?;
    Ok(value)
}

/// Plans the sealed desired catalog against one realization.
///
/// # Errors
/// Returns an error when a sealed row breaks the state and value binding.
pub(crate) fn plan_catalog(realization: &str) -> Result<Datum, WorldError> {
    plan(realization, service_facts::DESIRED)
}

/// Plans one desired value against one realization.
///
/// # Errors
/// Returns an error when a sealed row breaks the state and value binding.
pub(crate) fn plan_one(
    realization: &str,
    selector: &str,
    desired: &str,
) -> Result<Datum, WorldError> {
    plan(realization, &[(selector, desired)])
}

/// Plans desired values against readings from one realization.
///
/// Already-true current matches are omitted. Divergent and unknown facts are
/// retained. This function does not write a reading.
///
/// # Errors
/// Returns an error when a request repeats a selector or a sealed row breaks
/// the state and value binding.
pub(crate) fn plan(realization: &str, requests: &[(&str, &str)]) -> Result<Datum, WorldError> {
    let Some(mode) = mode_of(realization) else {
        return Ok(gap(realization, "", "unknown-realization"));
    };
    let mut ordered: Vec<(&str, &str)> = requests.to_vec();
    ordered.sort_by(|left, right| left.0.cmp(right.0));
    for pair in ordered.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(WorldError::Qualification(format!(
                "duplicate selector {}",
                pair[0].0
            )));
        }
    }
    let mut actions = Vec::new();
    let mut omitted = Vec::new();
    let mut retained = Vec::new();
    let mut ports = BTreeSet::new();
    let mut reads = BTreeSet::new();
    for (selector, desired) in ordered {
        let reading = read_one(mode, selector)?;
        if reading.called {
            ports.insert(reading.port.clone());
            reads.insert(selector.to_owned());
        }
        match judge(&reading.state, reading.value.as_deref(), desired) {
            "already-true" => omitted.push(selector.to_owned()),
            "divergent" => {
                actions.push(action_datum(selector));
                retained.push(fact_datum(&reading, desired, "divergent"));
            }
            _ => retained.push(fact_datum(&reading, desired, "unknown")),
        }
    }
    Ok(plan_datum(
        realization,
        actions,
        omitted,
        retained,
        ports,
        reads,
    ))
}

#[derive(Clone, Copy)]
enum Mode {
    Model,
    Native,
    Real,
}

fn mode_of(realization: &str) -> Option<Mode> {
    match realization {
        "model" => Some(Mode::Model),
        "native" => Some(Mode::Native),
        "real" => Some(Mode::Real),
        _ => None,
    }
}

fn read_one(mode: Mode, selector: &str) -> Result<Reading, WorldError> {
    let identity = identity(selector);
    match mode {
        Mode::Native => Ok(refusal(
            selector,
            identity,
            "native",
            POLICY_NATIVE,
            PROVENANCE_NATIVE,
        )),
        Mode::Real => Ok(refusal(
            selector,
            identity,
            "real",
            POLICY_REAL,
            PROVENANCE_REAL,
        )),
        Mode::Model => model_reading(selector, identity),
    }
}

fn refusal(
    selector: &str,
    identity: Option<(&str, &str)>,
    port: &str,
    policy: &'static str,
    provenance: &str,
) -> Reading {
    Reading {
        kind: identity
            .map(|(_, kind)| kind.to_owned())
            .unwrap_or_else(|| "unknown".to_owned()),
        policy,
        port: port.to_owned(),
        provenance: provenance.to_owned(),
        selector: selector.to_owned(),
        state: "unavailable".to_owned(),
        value: None,
        called: false,
    }
}

fn model_reading(selector: &str, identity: Option<(&str, &str)>) -> Result<Reading, WorldError> {
    let Some((port, kind)) = identity else {
        return Ok(Reading {
            kind: "unknown".to_owned(),
            policy: POLICY_MODEL,
            port: "none".to_owned(),
            provenance: "selector-miss".to_owned(),
            selector: selector.to_owned(),
            state: "absent".to_owned(),
            value: None,
            called: false,
        });
    };
    let found = service_facts::FACTS
        .iter()
        .find(|fact| fact.selector == selector);
    if let Some(fact) = found {
        bind_fact(fact, port, kind)?;
        return Ok(Reading {
            kind: kind.to_owned(),
            policy: POLICY_MODEL,
            port: port.to_owned(),
            provenance: format!("model/{selector}"),
            selector: selector.to_owned(),
            state: fact.state.to_owned(),
            value: fact.value.map(str::to_owned),
            called: true,
        });
    }
    Ok(Reading {
        kind: kind.to_owned(),
        policy: POLICY_MODEL,
        port: port.to_owned(),
        provenance: "selector-miss".to_owned(),
        selector: selector.to_owned(),
        state: "absent".to_owned(),
        value: None,
        called: true,
    })
}

fn bind_fact(fact: &service_facts::Fact, port: &str, kind: &str) -> Result<(), WorldError> {
    let Some((fact_port, fact_kind)) = identity(fact.selector) else {
        return Err(WorldError::Qualification(format!(
            "selector {} has no service port",
            fact.selector
        )));
    };
    if fact_port != port || fact_kind != kind {
        return Err(WorldError::Qualification(format!(
            "selector {} disagrees with its port",
            fact.selector
        )));
    }
    let carries = matches!(fact.state, "current" | "stale" | "partial" | "lagging");
    let withholds = matches!(fact.state, "denied" | "uncertain");
    if !carries && !withholds {
        return Err(WorldError::Qualification(format!(
            "unknown service state {}",
            fact.state
        )));
    }
    if carries != fact.value.is_some() {
        return Err(WorldError::Qualification(format!(
            "state {} on {} does not bind a service value",
            fact.state, fact.selector
        )));
    }
    Ok(())
}

fn identity(selector: &str) -> Option<(&'static str, &'static str)> {
    let mut parts = selector.split('/');
    let port = parts.next()?;
    let second = parts.next()?;
    let third = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let kind = match (port, second, third) {
        ("git", "branch", Some(name)) if !name.is_empty() => "branch",
        ("git", "tag", Some(name)) if !name.is_empty() => "tag",
        ("git", "direct", Some(name)) if !name.is_empty() => "direct",
        ("git", "peeled", Some(name)) if !name.is_empty() => "peeled",
        ("registry", "checksum", Some(name)) if !name.is_empty() => "checksum",
        ("ci", name, None) if !name.is_empty() => "ci",
        ("mirror", name, None) if !name.is_empty() => "mirror",
        _ => return None,
    };
    let fact_port = match kind {
        "checksum" => "registry",
        "ci" => "ci",
        "mirror" => "mirror",
        _ => "git",
    };
    Some((fact_port, kind))
}

fn judge(state: &str, observed: Option<&str>, desired: &str) -> &'static str {
    if state == "current" && observed == Some(desired) {
        "already-true"
    } else if state == "current" {
        "divergent"
    } else {
        "unknown"
    }
}

fn snapshot_datum(realization: &str, selector: &str, reading: &Reading) -> Datum {
    let (ports, reads) = if reading.called {
        (vec![reading.port.clone()], vec![reading.selector.clone()])
    } else {
        (Vec::new(), Vec::new())
    };
    Datum::Node {
        tag: Symbol::qualified("world", "service-snapshot-v1"),
        fields: vec![
            (Symbol::new("effects"), query::count_datum(0)),
            (
                Symbol::new("observations"),
                Datum::Vector(vec![row_datum(reading)]),
            ),
            (Symbol::new("ports"), string_vector(ports)),
            (Symbol::new("reads"), string_vector(reads)),
            (
                Symbol::new("realization"),
                Datum::String(realization.to_owned()),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("selector"), Datum::String(selector.to_owned())),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn plan_datum(
    realization: &str,
    actions: Vec<Datum>,
    omitted: Vec<String>,
    retained: Vec<Datum>,
    ports: BTreeSet<String>,
    reads: BTreeSet<String>,
) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "service-plan-v1"),
        fields: vec![
            (Symbol::new("actions"), Datum::Vector(actions)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("omitted"), string_vector(omitted)),
            (
                Symbol::new("ports"),
                string_vector(ports.into_iter().collect()),
            ),
            (
                Symbol::new("reads"),
                string_vector(reads.into_iter().collect()),
            ),
            (
                Symbol::new("realization"),
                Datum::String(realization.to_owned()),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("retained"), Datum::Vector(retained)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn row_datum(reading: &Reading) -> Datum {
    let mut fields = vec![
        (Symbol::new("kind"), Datum::String(reading.kind.clone())),
        (
            Symbol::new("policy"),
            Datum::String(reading.policy.to_owned()),
        ),
        (Symbol::new("port"), Datum::String(reading.port.clone())),
        (
            Symbol::new("provenance"),
            Datum::String(reading.provenance.clone()),
        ),
        (
            Symbol::new("selector"),
            Datum::String(reading.selector.clone()),
        ),
        (Symbol::new("state"), Datum::String(reading.state.clone())),
    ];
    if let Some(value) = &reading.value {
        fields.push((Symbol::new("value"), Datum::String(value.clone())));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "service-observation-v1"),
        fields,
    }
}

fn fact_datum(reading: &Reading, desired: &str, class: &str) -> Datum {
    let mut fields = vec![
        (Symbol::new("class"), Datum::String(class.to_owned())),
        (Symbol::new("desired"), Datum::String(desired.to_owned())),
        (Symbol::new("kind"), Datum::String(reading.kind.clone())),
        (Symbol::new("port"), Datum::String(reading.port.clone())),
        (
            Symbol::new("provenance"),
            Datum::String(reading.provenance.clone()),
        ),
        (
            Symbol::new("selector"),
            Datum::String(reading.selector.clone()),
        ),
        (Symbol::new("state"), Datum::String(reading.state.clone())),
    ];
    if let Some(value) = &reading.value {
        fields.push((Symbol::new("value"), Datum::String(value.clone())));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "service-fact-v1"),
        fields,
    }
}

fn action_datum(selector: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "service-action-v1"),
        fields: vec![
            (Symbol::new("class"), Datum::String("divergent".to_owned())),
            (Symbol::new("selector"), Datum::String(selector.to_owned())),
        ],
    }
}

fn gap(realization: &str, selector: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "service-gap-v1"),
        fields: vec![
            (Symbol::new("effects"), query::count_datum(0)),
            (
                Symbol::new("realization"),
                Datum::String(realization.to_owned()),
            ),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("selector"), Datum::String(selector.to_owned())),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn string_vector(values: Vec<String>) -> Datum {
    Datum::Vector(values.into_iter().map(Datum::String).collect())
}
