// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded responsibility-transfer command.
//!
//! The caller supplies one observed document. Retirement of a previous
//! path requires caller-zero evidence and a zero caller count. Cargo,
//! Git, validation, and operator recovery stay under their named roles.
//! A native request, a native or publication path segment, or a model
//! credential refuses before any effect. The command starts no
//! observation and does not derive a completion seal.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

const HEADER: &str = "transfer world/responsibility-transfer-v1";
const KEYS: [&str; 11] = [
    "responsibility",
    "scope",
    "entrypoint",
    "previous",
    "disposition",
    "callers",
    "evidence",
    "role",
    "grant",
    "provider",
    "credential",
];
const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

struct Fault {
    responsibility: String,
    reason: &'static str,
}

struct Fields {
    responsibility: String,
    scope: String,
    entrypoint: String,
    previous: String,
    disposition: String,
    callers: u64,
    evidence: String,
    role: String,
    grant: String,
    provider: String,
    credential: String,
}

struct Admitted {
    responsibility: String,
    disposition: &'static str,
    role: String,
    evidence: String,
}

/// Admits every portable responsibility transfer in `document`.
///
/// A later identical document returns the same record. The command reads
/// only that document.
///
/// # Errors
///
/// Returns an error when the transfer record cannot be represented.
pub(crate) fn run(document: &str) -> Result<Datum, WorldError> {
    if leaked(document) {
        return Ok(gap("", "private-leakage"));
    }
    if native_request(document) {
        return Ok(gap(&first_responsibility(document), "native-request"));
    }
    if let Some(responsibility) = native_scope(document) {
        return Ok(gap(&responsibility, "native-request"));
    }
    match admit_document(document) {
        Ok(rows) => Ok(success(&rows)),
        Err(fault) => Ok(gap(&fault.responsibility, fault.reason)),
    }
}

fn native_request(document: &str) -> bool {
    document
        .lines()
        .any(|line| trimmed(line) == "request native")
}

fn native_scope(document: &str) -> Option<String> {
    for block in raw_blocks(document) {
        let native = block.iter().any(|line| {
            matches!(
                line.strip_prefix("scope ").map(str::trim),
                Some("linux" | "native")
            )
        });
        if native {
            let responsibility = block
                .iter()
                .find_map(|line| line.strip_prefix("responsibility ").map(str::trim))
                .unwrap_or("");
            return Some(responsibility.to_owned());
        }
    }
    None
}

fn raw_blocks(document: &str) -> Vec<Vec<&str>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in document.lines().map(trimmed) {
        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn first_responsibility(document: &str) -> String {
    document
        .lines()
        .find_map(|line| trimmed(line).strip_prefix("responsibility "))
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn admit_document(document: &str) -> Result<Vec<Admitted>, Fault> {
    let lines: Vec<&str> = document.lines().map(trimmed).collect();
    let mut index = 0;
    while index < lines.len() && lines[index].is_empty() {
        index += 1;
    }
    if index >= lines.len() {
        return Err(fault("", "missing-transfer"));
    }
    if lines[index] != HEADER {
        return Err(fault("", "header"));
    }
    let mut rows = Vec::new();
    for block in blocks(&lines[index + 1..])? {
        rows.push(admit_block(&block)?);
    }
    Ok(rows)
}

fn blocks(lines: &[&str]) -> Result<Vec<Vec<String>>, Fault> {
    let mut found = Vec::new();
    let mut current = Vec::new();
    for line in lines {
        if line.is_empty() {
            if !current.is_empty() {
                found.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push((*line).to_owned());
    }
    if !current.is_empty() {
        found.push(current);
    }
    if found.is_empty() {
        return Err(fault("", "missing-transfer"));
    }
    Ok(found)
}

fn admit_block(lines: &[String]) -> Result<Admitted, Fault> {
    let fields = parse_block(lines)?;
    let responsibility = fields.responsibility.as_str();
    if fields.scope == "linux" || fields.scope == "native" {
        return Err(fault(responsibility, "native-request"));
    }
    if fields.scope != "portable" {
        return Err(fault(responsibility, "scope"));
    }
    if native_segment(&fields.grant)
        || native_segment(&fields.provider)
        || native_segment(&fields.entrypoint)
        || native_segment(&fields.previous)
    {
        return Err(fault(responsibility, "native-authority"));
    }
    if fields.credential != "none" {
        return Err(fault(responsibility, "model-credential"));
    }
    if !fields.provider.starts_with("model/") {
        return Err(fault(responsibility, "provider"));
    }
    if !known_responsibility(responsibility) {
        return Err(fault(responsibility, "responsibility"));
    }
    match fields.disposition.as_str() {
        "retire" => admit_retire(&fields),
        "retain" => admit_retain(&fields),
        "open" => admit_open(&fields),
        _ => Err(fault(responsibility, "disposition")),
    }
}

fn parse_block(lines: &[String]) -> Result<Fields, Fault> {
    let mut values: [Option<String>; 11] = [
        None, None, None, None, None, None, None, None, None, None, None,
    ];
    let mut responsibility = String::new();
    for line in lines {
        let Some((key, value)) = split_field(line) else {
            return Err(fault(&responsibility, "field"));
        };
        let Some(index) = KEYS.iter().position(|name| *name == key) else {
            return Err(fault(&responsibility, "field"));
        };
        if values[index].is_some() {
            return Err(fault(&responsibility, "duplicate"));
        }
        if key == "responsibility" {
            responsibility = value.to_owned();
        }
        values[index] = Some(value.to_owned());
    }
    let mut taken: [String; 11] = std::array::from_fn(|_| String::new());
    for (index, key) in KEYS.iter().enumerate() {
        let Some(value) = values[index].clone() else {
            return Err(fault(&responsibility, key));
        };
        taken[index] = value;
    }
    let callers = parse_callers(&taken[5]).map_err(|()| fault(&responsibility, "callers"))?;
    Ok(Fields {
        responsibility: taken[0].clone(),
        scope: taken[1].clone(),
        entrypoint: taken[2].clone(),
        previous: taken[3].clone(),
        disposition: taken[4].clone(),
        callers,
        evidence: taken[6].clone(),
        role: taken[7].clone(),
        grant: taken[8].clone(),
        provider: taken[9].clone(),
        credential: taken[10].clone(),
    })
}

fn split_field(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(' ')?;
    let value = value.trim();
    if key.is_empty() || value.is_empty() || value.split_whitespace().nth(1).is_some() {
        return None;
    }
    Some((key, value))
}

fn parse_callers(value: &str) -> Result<u64, ()> {
    if value == "0" {
        return Ok(0);
    }
    if value.starts_with('0') || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    value.parse().map_err(|_| ())
}

fn admit_retire(fields: &Fields) -> Result<Admitted, Fault> {
    let responsibility = fields.responsibility.as_str();
    if ordinary(&fields.previous) || ordinary(&fields.role) {
        return Err(fault(responsibility, "ordinary-tool"));
    }
    if fields.role != "none" {
        return Err(fault(responsibility, "role"));
    }
    if fields.previous == "none" {
        return Err(fault(responsibility, "previous"));
    }
    if fields.callers != 0 {
        return Err(fault(responsibility, "callers"));
    }
    let expected = format!("caller-zero/{}", fields.previous);
    if fields.evidence != expected {
        return Err(fault(responsibility, "caller-zero"));
    }
    Ok(admitted(fields, "retire"))
}

fn admit_retain(fields: &Fields) -> Result<Admitted, Fault> {
    let responsibility = fields.responsibility.as_str();
    if !ordinary(&fields.role) {
        return Err(fault(responsibility, "role"));
    }
    if fields.previous != fields.role {
        return Err(fault(responsibility, "previous"));
    }
    if fields.evidence != "none" {
        return Err(fault(responsibility, "evidence"));
    }
    Ok(admitted(fields, "retain"))
}

fn admit_open(fields: &Fields) -> Result<Admitted, Fault> {
    let responsibility = fields.responsibility.as_str();
    if fields.previous != "none" {
        return Err(fault(responsibility, "previous"));
    }
    if fields.role != "none" {
        return Err(fault(responsibility, "role"));
    }
    if fields.evidence != "none" {
        return Err(fault(responsibility, "evidence"));
    }
    if fields.callers != 0 {
        return Err(fault(responsibility, "callers"));
    }
    Ok(admitted(fields, "open"))
}

fn admitted(fields: &Fields, disposition: &'static str) -> Admitted {
    Admitted {
        responsibility: fields.responsibility.clone(),
        disposition,
        role: fields.role.clone(),
        evidence: fields.evidence.clone(),
    }
}

fn success(rows: &[Admitted]) -> Datum {
    node(
        "transfer-v1",
        vec![
            ("effects", query::count_datum(0)),
            ("evidence", string(list(rows, |row| listed(&row.evidence)))),
            ("opened", query::count_datum(count(rows, "open"))),
            ("resident", Datum::Bool(true)),
            (
                "responsibilities",
                string(list(rows, |row| Some(row.responsibility.as_str()))),
            ),
            ("retained", query::count_datum(count(rows, "retain"))),
            ("retired", query::count_datum(count(rows, "retire"))),
            ("roles", string(list(rows, |row| listed(&row.role)))),
            ("scope", string("portable")),
            ("seal", string("unclaimed")),
            ("subprocesses", query::count_datum(0)),
            ("transfers", query::count_datum(rows.len() as u64)),
        ],
    )
}

fn gap(responsibility: &str, reason: &str) -> Datum {
    node(
        "transfer-gap-v1",
        vec![
            ("effects", query::count_datum(0)),
            ("reason", string(reason)),
            ("resident", Datum::Bool(true)),
            ("responsibility", string(shown(responsibility))),
            ("seal", string("unclaimed")),
            ("subprocesses", query::count_datum(0)),
        ],
    )
}

fn count(rows: &[Admitted], disposition: &str) -> u64 {
    rows.iter()
        .filter(|row| row.disposition == disposition)
        .count() as u64
}

fn list(rows: &[Admitted], pick: impl Fn(&Admitted) -> Option<&str>) -> String {
    rows.iter().filter_map(pick).collect::<Vec<_>>().join(",")
}

fn listed(value: &str) -> Option<&str> {
    if value == "none" { None } else { Some(value) }
}

fn known_responsibility(value: &str) -> bool {
    matches!(
        value,
        "r0-witness"
            | "r1-explain"
            | "r2-observe"
            | "r3-prove"
            | "r4-converge"
            | "r5-land"
            | "r6-reconcile"
            | "r7-deliver"
            | "r8-conduct"
            | "r9-succeed"
    )
}

fn ordinary(value: &str) -> bool {
    matches!(value, "cargo" | "git" | "validation" | "operator-recovery")
}

fn native_segment(value: &str) -> bool {
    value
        .split('/')
        .any(|part| matches!(part, "linux" | "native" | "publication" | "sudo"))
}

fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn fault(responsibility: &str, reason: &'static str) -> Fault {
    Fault {
        responsibility: shown(responsibility),
        reason,
    }
}

fn trimmed(line: &str) -> &str {
    line.trim().trim_end_matches('\r')
}
