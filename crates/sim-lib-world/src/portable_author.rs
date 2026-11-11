// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authoring gate for the portable phase union.
//!
//! The leaf calls the loaded author command. One selected family is listed,
//! shown, linted, selected, diffed, and compiled. A consulted or superseded
//! sibling does not satisfy a requirement the selected source omits. A missing
//! section refuses in the parser. A decision that names begin authority
//! refuses. A private value is not copied.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/author-v1";
const GAP: &str = "world/author-gap-v1";
const PRIVATE: &str = "private-material:s3cret-token";

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

const LABELS: &[&str] = &[
    "selected",
    "consulted",
    "superseded",
    "argument",
    "work",
    "gate",
    "ratchet",
    "owner",
    "checker",
    "authority",
    "private",
];

#[path = "portable_author_expect.rs"]
mod expect;
#[path = "portable_author_parse.rs"]
mod parse;

struct Capture {
    rows: Vec<Row>,
    stable: bool,
    exports: bool,
}

struct Row {
    label: &'static str,
    value: Datum,
}

pub(super) struct OwnedAuthor {
    case: String,
    class: String,
    members: String,
    shown: String,
    lint: String,
    selection: String,
    changed: String,
    round_trip: String,
    shapes: String,
    compiled: String,
    consultation: String,
    bytes: String,
    proof: String,
    phase: String,
    uncertain: String,
    closed: String,
    grounded: String,
    claims: String,
    entries: String,
    postimage: String,
}

pub(super) struct OwnedGap {
    case: String,
    reason: String,
    evidence: String,
}

pub(super) fn author_leaf() -> Result<ProofLeaf, WorldError> {
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
    let mut rows = Vec::with_capacity(LABELS.len());
    let mut stable = true;
    for label in LABELS {
        let (value, same) = call_pair(command, &argv(label))?;
        stable &= same;
        rows.push(Row { label, value });
    }
    Ok(Capture {
        rows,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/24".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn argv(label: &str) -> Vec<String> {
    let case = if label == "private" { PRIVATE } else { label };
    vec!["sim".to_owned(), "author".to_owned(), case.to_owned()]
}

fn call_pair(command: &WorldCommandLib, argv: &[String]) -> Result<(Datum, bool), WorldError> {
    let first = command.run(argv)?;
    let second = command.run(argv)?;
    let same = first == second;
    Ok((first, same))
}

fn actual_deck(capture: &Capture) -> String {
    let (effects, subprocesses, resident) = meter(capture);
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    for row in &capture.rows {
        push_row(&mut lines, row);
    }
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    if capture.rows.iter().any(|row| contains_private(&row.value)) {
        lines.push("leaked".to_owned());
    }
    lines.join("\n")
}

fn push_row(lines: &mut Vec<String>, row: &Row) {
    if let Some(author) = parse::parsed_author(&row.value) {
        lines.push(expect::author_line(&author));
        return;
    }
    if let Some(gap) = parse::parsed_gap(&row.value) {
        lines.push(expect::gap_line(row.label, &gap));
        return;
    }
    lines.push(format!("row {} missing", row.label));
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for row in &capture.rows {
        if let Some(author) = parse::parsed_author(&row.value) {
            claims.insert(expect::author_claim(&author));
        } else if let Some(gap) = parse::parsed_gap(&row.value) {
            claims.insert(format!("gap {} {}", row.label, gap.reason));
        }
    }
    claims
}

fn meter(capture: &Capture) -> (String, String, String) {
    let mut effects = "0".to_owned();
    let mut subprocesses = "0".to_owned();
    let mut resident = "true".to_owned();
    for row in &capture.rows {
        let effect = field_count(&row.value, "effects");
        if effect != "0" {
            effects = effect;
        }
        let subprocess = field_count(&row.value, "subprocesses");
        if subprocess != "0" {
            subprocesses = subprocess;
        }
        let live = field_bool(&row.value, "resident");
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
#[path = "portable_author_tests.rs"]
mod tests;
