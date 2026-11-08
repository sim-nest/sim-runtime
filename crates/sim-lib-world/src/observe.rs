// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit observation of one sealed model batch through domain ports.
//!
//! Filesystem, process, Git, registry, and CI facts stay with the port named
//! by the selector. A caller-named batch reads the ports declared by its rows,
//! and an undeclared provider returns a gap. Semantic identity binds freshness,
//! provenance, and policy.
//! Diagnostic envelopes stay outside that identity. A native realization
//! returns unavailable and does not read the model batch.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::provider::content_id;
use crate::query;

const POLICY_MODEL: &str = "policy/explicit-model-v1";
const POLICY_NATIVE: &str = "policy/no-native-fallback-v1";
const PORTS: [&str; 5] = ["ci", "filesystem", "git", "process", "registry"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Semantic<'a> {
    port: &'a str,
    selector: &'a str,
    status: &'a str,
    value: Option<&'a str>,
}

#[cfg(test)]
const MODEL: &[Semantic] = &[
    Semantic {
        port: "ci",
        selector: "ci/run",
        status: "uncertain",
        value: None,
    },
    Semantic {
        port: "filesystem",
        selector: "filesystem/readme",
        status: "observed",
        value: Some("verified-source"),
    },
    Semantic {
        port: "filesystem",
        selector: "filesystem/secret",
        status: "denied",
        value: None,
    },
    Semantic {
        port: "git",
        selector: "git/HEAD",
        status: "timed-out",
        value: None,
    },
    Semantic {
        port: "process",
        selector: "process/probe",
        status: "unavailable",
        value: None,
    },
    Semantic {
        port: "registry",
        selector: "registry/index",
        status: "absent",
        value: None,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum Kind {
    Model,
    Revised,
    Envelope,
    Native,
}

struct Description {
    semantic: Datum,
    semantic_id: String,
    selector: String,
    status: String,
    envelope: String,
    matched: bool,
    port_called: Option<String>,
    read: Option<String>,
    value: Option<String>,
    checksum: Option<String>,
    freshness: String,
    policy: String,
    port: String,
    provenance: String,
}

#[path = "observe_rows.rs"]
mod rows;

pub(crate) use rows::{diff_documents, snapshot_document};

/// Reads one selector from an explicit batch through its domain port.
///
/// # Errors
/// Returns an error when a sealed row breaks the status and value binding.
#[cfg(test)]
pub(crate) fn snapshot(batch: &str, selector: &str) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap("empty-selector", batch, selector));
    }
    let Some(source) = source_of(batch) else {
        return Ok(gap("unknown-batch", batch, selector));
    };
    if missing_capability(&source, selector) {
        return Ok(gap("missing-capability", batch, selector));
    }
    let described = describe_source(&source, selector)?;
    Ok(snapshot_datum(batch, selector, &described))
}

/// Compares one selector across two explicit batches.
///
/// Equal semantic batches return before any domain port is called.
///
/// # Errors
/// Returns an error when a sealed row breaks the status and value binding.
#[cfg(test)]
pub(crate) fn diff(before: &str, after: &str, selector: &str) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap("empty-selector", before, selector));
    }
    let Some(left_source) = source_of(before) else {
        return Ok(gap("unknown-batch", before, selector));
    };
    let Some(right_source) = source_of(after) else {
        return Ok(gap("unknown-batch", after, selector));
    };
    if missing_capability(&left_source, selector) || missing_capability(&right_source, selector) {
        return Ok(gap("missing-capability", before, selector));
    }
    let left = describe_source(&left_source, selector)?;
    let right = describe_source(&right_source, selector)?;
    if left.semantic == right.semantic {
        return Ok(diff_datum(before, after, selector, None, None));
    }
    Ok(diff_datum(
        before,
        after,
        selector,
        Some(&left),
        Some(&right),
    ))
}

#[cfg(test)]
fn kind_of(batch: &str) -> Option<Kind> {
    match batch {
        "model" => Some(Kind::Model),
        "revised" => Some(Kind::Revised),
        "envelope" => Some(Kind::Envelope),
        "native" => Some(Kind::Native),
        _ => None,
    }
}

#[cfg(test)]
enum Source {
    Kind(Kind),
    Rows(&'static [crate::fixture::Row]),
}

#[cfg(test)]
fn source_of(batch: &str) -> Option<Source> {
    if let Some(rows) = crate::fixture::rows(batch) {
        return Some(Source::Rows(rows));
    }
    kind_of(batch).map(Source::Kind)
}

#[cfg(test)]
fn missing_capability(source: &Source, selector: &str) -> bool {
    let Source::Rows(rows) = source else {
        return false;
    };
    let Some((prefix, _)) = selector.split_once('/') else {
        return true;
    };
    !rows.iter().any(|row| row.port == prefix)
}

#[cfg(test)]
fn describe_source(source: &Source, selector: &str) -> Result<Description, WorldError> {
    match source {
        Source::Kind(kind) => describe(*kind, selector),
        Source::Rows(rows) => {
            let found = rows
                .iter()
                .find(|row| row.selector == selector)
                .map(|row| Semantic {
                    port: row.port,
                    selector: row.selector,
                    status: row.status,
                    value: row.value,
                });
            let declared = selector.split_once('/').map(|(prefix, _)| prefix);
            describe_row(found, selector, "model-log", declared)
        }
    }
}

#[cfg(test)]
fn describe(kind: Kind, selector: &str) -> Result<Description, WorldError> {
    if kind == Kind::Native {
        return native_description(selector);
    }
    describe_row(sealed(kind, selector), selector, envelope_text(kind), None)
}

fn describe_row(
    found: Option<Semantic<'_>>,
    selector: &str,
    envelope: &str,
    declared_port: Option<&str>,
) -> Result<Description, WorldError> {
    if let Some(row) = found {
        bind_row(row)?;
        if let Some(prefix) = declared_port
            && row.port != prefix
        {
            return Err(WorldError::Qualification(format!(
                "selector {selector} escapes its port"
            )));
        } else if declared_port.is_none() && known_port(selector) != Some(row.port) {
            return Err(WorldError::Qualification(format!(
                "selector {selector} escapes its port"
            )));
        }
    }
    let port = declared_port.or_else(|| known_port(selector));
    let status = found.map(|row| row.status).unwrap_or("absent");
    let value = found.and_then(|row| row.value);
    let provenance = match found {
        Some(row) if row.status == "observed" => format!("verified-source/{selector}"),
        Some(_) => format!("model/{selector}"),
        None => "selector-miss".to_owned(),
    };
    let port_name = found
        .map(|row| row.port)
        .or(port)
        .unwrap_or("none")
        .to_owned();
    let checksum = match value {
        Some(body) => Some(hex_id(Datum::String(body.to_owned()))?),
        None => None,
    };
    let freshness = freshness_of(selector, status, value, checksum.as_deref())?;
    let semantic = semantic_node(&Bound {
        port: &port_name,
        selector,
        status,
        value,
        freshness: &freshness,
        provenance: &provenance,
        policy: POLICY_MODEL,
        checksum: checksum.as_deref(),
    });
    let semantic_id = hex_id(semantic.clone())?;
    Ok(Description {
        semantic,
        semantic_id,
        selector: selector.to_owned(),
        status: status.to_owned(),
        envelope: envelope.to_owned(),
        matched: found.is_some(),
        port_called: port.map(str::to_owned),
        read: port.map(|_| selector.to_owned()),
        value: value.map(str::to_owned),
        checksum,
        freshness,
        policy: POLICY_MODEL.to_owned(),
        port: port_name,
        provenance,
    })
}

fn native_description(selector: &str) -> Result<Description, WorldError> {
    let freshness = freshness_of(selector, "unavailable", None, None)?;
    let provenance = "native-capability-missing".to_owned();
    let semantic = semantic_node(&Bound {
        port: "native",
        selector,
        status: "unavailable",
        value: None,
        freshness: &freshness,
        provenance: &provenance,
        policy: POLICY_NATIVE,
        checksum: None,
    });
    let semantic_id = hex_id(semantic.clone())?;
    Ok(Description {
        semantic,
        semantic_id,
        selector: selector.to_owned(),
        status: "unavailable".to_owned(),
        envelope: "native-refused".to_owned(),
        matched: false,
        port_called: None,
        read: None,
        value: None,
        checksum: None,
        freshness,
        policy: POLICY_NATIVE.to_owned(),
        port: "native".to_owned(),
        provenance,
    })
}

fn bind_row(row: Semantic<'_>) -> Result<(), WorldError> {
    let known = matches!(
        row.status,
        "observed" | "denied" | "unavailable" | "timed-out" | "absent" | "uncertain"
    );
    if !known {
        return Err(WorldError::Qualification(format!(
            "unknown observation status {}",
            row.status
        )));
    }
    if (row.status == "observed") != row.value.is_some() {
        return Err(WorldError::Qualification(format!(
            "status {} on {} does not bind a semantic value",
            row.status, row.selector
        )));
    }
    Ok(())
}

#[cfg(test)]
fn sealed(kind: Kind, selector: &str) -> Option<Semantic<'_>> {
    match kind {
        Kind::Revised => revised(selector).or_else(|| base(selector)),
        Kind::Model | Kind::Envelope => base(selector),
        Kind::Native => None,
    }
}

#[cfg(test)]
fn base(selector: &str) -> Option<Semantic<'_>> {
    MODEL.iter().copied().find(|row| row.selector == selector)
}

#[cfg(test)]
fn revised(selector: &str) -> Option<Semantic<'_>> {
    match selector {
        "git/HEAD" => Some(Semantic {
            port: "git",
            selector: "git/HEAD",
            status: "observed",
            value: Some("abc"),
        }),
        "process/probe" => Some(Semantic {
            port: "process",
            selector: "process/probe",
            status: "denied",
            value: None,
        }),
        _ => None,
    }
}

fn known_port(selector: &str) -> Option<&'static str> {
    let prefix = selector.split_once('/')?.0;
    PORTS.into_iter().find(|port| *port == prefix)
}

#[cfg(test)]
fn envelope_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Envelope => "envelope-log",
        Kind::Native => "native-refused",
        Kind::Model | Kind::Revised => "model-log",
    }
}

fn freshness_of(
    selector: &str,
    status: &str,
    value: Option<&str>,
    checksum: Option<&str>,
) -> Result<String, WorldError> {
    let mut fields = vec![
        (Symbol::new("selector"), Datum::String(selector.to_owned())),
        (Symbol::new("status"), Datum::String(status.to_owned())),
    ];
    if let Some(checksum) = checksum {
        fields.push((Symbol::new("checksum"), Datum::String(checksum.to_owned())));
    }
    if let Some(value) = value {
        fields.push((Symbol::new("value"), Datum::String(value.to_owned())));
    }
    hex_id(Datum::Node {
        tag: Symbol::qualified("world", "observation-freshness-v1"),
        fields,
    })
}

struct Bound<'a> {
    port: &'a str,
    selector: &'a str,
    status: &'a str,
    value: Option<&'a str>,
    freshness: &'a str,
    provenance: &'a str,
    policy: &'a str,
    checksum: Option<&'a str>,
}

fn semantic_node(bound: &Bound<'_>) -> Datum {
    let mut fields = vec![
        (
            Symbol::new("freshness"),
            Datum::String(bound.freshness.to_owned()),
        ),
        (
            Symbol::new("policy"),
            Datum::String(bound.policy.to_owned()),
        ),
        (Symbol::new("port"), Datum::String(bound.port.to_owned())),
        (
            Symbol::new("provenance"),
            Datum::String(bound.provenance.to_owned()),
        ),
        (
            Symbol::new("selector"),
            Datum::String(bound.selector.to_owned()),
        ),
        (
            Symbol::new("status"),
            Datum::String(bound.status.to_owned()),
        ),
    ];
    if let Some(checksum) = bound.checksum {
        fields.push((Symbol::new("checksum"), Datum::String(checksum.to_owned())));
    }
    if let Some(value) = bound.value {
        fields.push((Symbol::new("value"), Datum::String(value.to_owned())));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "semantic-observation-v1"),
        fields,
    }
}

fn snapshot_datum(batch: &str, selector: &str, described: &Description) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "snapshot-v1"),
        fields: vec![
            (Symbol::new("batch"), Datum::String(batch.to_owned())),
            (Symbol::new("effects"), query::count_datum(0)),
            (
                Symbol::new("observations"),
                Datum::Vector(vec![row_datum(described)]),
            ),
            (
                Symbol::new("ports"),
                string_vector(called(&described.port_called)),
            ),
            (Symbol::new("reads"), string_vector(called(&described.read))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("selector"), Datum::String(selector.to_owned())),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn diff_datum(
    before: &str,
    after: &str,
    selector: &str,
    left: Option<&Description>,
    right: Option<&Description>,
) -> Datum {
    let mut fields = vec![(Symbol::new("after"), Datum::String(after.to_owned()))];
    if let Some(right) = right {
        fields.push((
            Symbol::new("after-status"),
            Datum::String(right.status.clone()),
        ));
        push_optional(&mut fields, "after-value", right.value.as_deref());
    }
    fields.push((Symbol::new("before"), Datum::String(before.to_owned())));
    if let Some(left) = left {
        fields.push((
            Symbol::new("before-status"),
            Datum::String(left.status.clone()),
        ));
        push_optional(&mut fields, "before-value", left.value.as_deref());
    }
    let changed = left.is_some();
    let (ports, reads) = match (left, right) {
        (Some(left), Some(right)) => {
            let mut ports = called(&left.port_called);
            ports.extend(called(&right.port_called));
            let mut reads = called(&left.read);
            reads.extend(called(&right.read));
            (ports, reads)
        }
        _ => (Vec::new(), Vec::new()),
    };
    fields.extend([
        (Symbol::new("changed"), Datum::Bool(changed)),
        (Symbol::new("effects"), query::count_datum(0)),
        (Symbol::new("ports"), string_vector(ports)),
        (Symbol::new("reads"), string_vector(reads)),
        (Symbol::new("resident"), Datum::Bool(true)),
        (Symbol::new("selector"), Datum::String(selector.to_owned())),
        (Symbol::new("subprocesses"), query::count_datum(0)),
    ]);
    Datum::Node {
        tag: Symbol::qualified("world", "observation-diff-v1"),
        fields,
    }
}

fn row_datum(described: &Description) -> Datum {
    let mut fields = Vec::new();
    push_optional(&mut fields, "checksum", described.checksum.as_deref());
    fields.push((
        Symbol::new("envelope"),
        Datum::String(described.envelope.clone()),
    ));
    fields.push((
        Symbol::new("freshness"),
        Datum::String(described.freshness.clone()),
    ));
    fields.push((Symbol::new("matched"), Datum::Bool(described.matched)));
    fields.push((
        Symbol::new("policy"),
        Datum::String(described.policy.clone()),
    ));
    fields.push((Symbol::new("port"), Datum::String(described.port.clone())));
    fields.push((
        Symbol::new("provenance"),
        Datum::String(described.provenance.clone()),
    ));
    fields.push((
        Symbol::new("selector"),
        Datum::String(described.selector.clone()),
    ));
    fields.push((
        Symbol::new("semantic-id"),
        Datum::String(described.semantic_id.clone()),
    ));
    fields.push((
        Symbol::new("status"),
        Datum::String(described.status.clone()),
    ));
    push_optional(&mut fields, "value", described.value.as_deref());
    Datum::Node {
        tag: Symbol::qualified("world", "observation-v1"),
        fields,
    }
}

fn called(value: &Option<String>) -> Vec<String> {
    value.iter().cloned().collect()
}

fn push_optional(fields: &mut Vec<(Symbol, Datum)>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.push((Symbol::new(name), Datum::String(value.to_owned())));
    }
}

fn string_vector(values: Vec<String>) -> Datum {
    Datum::Vector(values.into_iter().map(Datum::String).collect())
}

fn gap(reason: &str, batch: &str, selector: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "observation-gap-v1"),
        fields: vec![
            (Symbol::new("batch"), Datum::String(batch.to_owned())),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("selector"), Datum::String(selector.to_owned())),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn hex_id(value: Datum) -> Result<String, WorldError> {
    let id = content_id(value)?;
    Ok(hex(&id.bytes))
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
