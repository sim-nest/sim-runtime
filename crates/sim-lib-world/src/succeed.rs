// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh succession over one exported model record.
//!
//! The caller supplies the export and one later change. Both are text.
//! The export names registry packages, one project, and declared inputs.
//! It does not name a checkout, a cache, a transcript, or a
//! process. The change is observed, sealed, proved, converged, landed,
//! delivered, and recovered. The repeated convergence and delivery do no
//! further work. A warm marker, a construction-fixture change, or an
//! unresolved hold stays open. Host reboot, power loss, and protected
//! production stay outside this record.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::fixture;
use crate::observe;
use crate::provider::content_id;
use crate::query;
use crate::seal;
use crate::service;

#[path = "succeed_ids.rs"]
mod ids;

const MAX_BYTES: usize = 8_192;
const MAX_PACKAGES: usize = 32;
const MAX_INPUTS: usize = 32;
const STATUSES: &[&str] = &[
    "observed",
    "denied",
    "unavailable",
    "timed-out",
    "absent",
    "uncertain",
];
const PRIVATE: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

struct Package {
    origin: String,
}

struct Row {
    port: String,
    selector: String,
    status: String,
    value: Option<String>,
}

struct Export {
    packages: Vec<Package>,
    project: String,
    inputs: Vec<Row>,
}

struct Change {
    id: String,
    project: String,
    row: Row,
}

struct Seen {
    status: String,
    semantic_id: String,
}

struct First {
    observed: u64,
    proved: u64,
    seal: u64,
    converge: u64,
    converge_second: u64,
    land: u64,
    deliver: u64,
    deliver_second: u64,
    effects: u64,
    subprocesses: u64,
    recovery_reads: u64,
    holds: u64,
    recovery: &'static str,
    closed: &'static str,
    preimage: String,
    postimage: String,
    input_ids: Vec<String>,
    change_id: String,
}

struct Host {
    effects: u64,
    subprocesses: u64,
}

struct Image {
    stored: Option<String>,
}

enum Stop {
    Gap(&'static str),
    Broken(String),
}

/// Applies one later change to an exported model record.
///
/// # Errors
/// Returns an error when a bound observation or its seal breaks contract.
pub(crate) fn run(export_text: &str, change_text: &str) -> Result<Datum, WorldError> {
    let host = host_of(export_text, change_text);
    if export_text.len() > MAX_BYTES || change_text.len() > MAX_BYTES {
        return Ok(gap("bound", &host));
    }
    if private_text(export_text) || private_text(change_text) {
        return Ok(gap("private-material", &host));
    }
    if warm(export_text) || warm(change_text) {
        return Ok(gap("warm-state", &host));
    }
    if claims_linux(export_text) || claims_linux(change_text) {
        return Ok(gap("linux-claim", &host));
    }
    if host.effects != 0 || host.subprocesses != 0 {
        return Ok(gap("host-effect", &host));
    }
    let export = match parse_export(export_text) {
        Ok(export) => export,
        Err(stop) => return stop_datum(stop, &host),
    };
    let change = match parse_change(change_text) {
        Ok(change) => change,
        Err(stop) => return stop_datum(stop, &host),
    };
    if let Some(reason) = admit(&export, &change) {
        return Ok(gap(reason, &host));
    }
    let first = match perform(&export, &change, &host) {
        Ok(first) => first,
        Err(stop) => return stop_datum(stop, &host),
    };
    Ok(record(&export, &change, &first))
}

fn host_of(export_text: &str, change_text: &str) -> Host {
    Host {
        effects: named(export_text, "effect") + named(change_text, "effect"),
        subprocesses: named(export_text, "subprocess") + named(change_text, "subprocess"),
    }
}

fn named(text: &str, kind: &str) -> u64 {
    text.lines()
        .filter(|line| line.split_whitespace().next() == Some(kind))
        .count() as u64
}

fn perform(export: &Export, change: &Change, host: &Host) -> Result<First, Stop> {
    let mut seen = Vec::new();
    let mut observed = 0_u64;
    for input in &export.inputs {
        seen.push(observe_row("export", input, &mut observed)?);
    }
    let change_seen = observe_row("change", &change.row, &mut observed)?;
    if seen
        .iter()
        .any(|row| row.semantic_id == change_seen.semantic_id)
    {
        return Err(Stop::Gap("empty-change"));
    }
    if construction_row(&change.row, &change_seen.semantic_id)? {
        return Err(Stop::Gap("fixture-change"));
    }
    let holds = hold_count(&seen) + hold_count(std::slice::from_ref(&change_seen));
    let proved = prove(&export.inputs, &change.row, &seen, &change_seen)?;
    let input_ids = seen
        .iter()
        .map(|row| row.semantic_id.clone())
        .collect::<Vec<_>>();
    let mut seals = 0_u64;
    let preimage = digest(&input_ids, &mut seals)?;
    let mut posted = input_ids.clone();
    posted.push(change_seen.semantic_id.clone());
    let postimage = digest(&posted, &mut seals)?;
    let mut first = First {
        observed,
        proved,
        seal: seals,
        converge: 0,
        converge_second: 0,
        land: 0,
        deliver: 0,
        deliver_second: 0,
        effects: host.effects,
        subprocesses: host.subprocesses,
        recovery_reads: 0,
        holds,
        recovery: "held",
        closed: "open",
        preimage,
        postimage,
        input_ids,
        change_id: change_seen.semantic_id,
    };
    if holds == 0 {
        let preimage = first.preimage.clone();
        first.recovery_reads = recover(export, &preimage)?;
        first.recovery = "match";
        first.closed = "portable";
    }
    let mut converged = Image { stored: None };
    let mut landed = Image { stored: None };
    let mut delivered = Image { stored: None };
    let candidate = (first.closed == "portable").then_some(first.postimage.as_str());
    first.converge = apply(&mut converged, candidate);
    first.land = apply(&mut landed, candidate);
    first.deliver = apply(&mut delivered, candidate);
    let again = converged.stored.clone();
    let deliver_again = delivered.stored.clone();
    first.converge_second = apply(&mut converged, again.as_deref());
    first.deliver_second = apply(&mut delivered, deliver_again.as_deref());
    Ok(first)
}

fn construction_row(change: &Row, found: &str) -> Result<bool, Stop> {
    if ids::matches_fixture(found)? {
        return Ok(true);
    }
    Ok(
        service::construction_rows().any(|(port, selector, state, value)| {
            change.port == port
                && change.selector == selector
                && change.status == state
                && change.value.as_deref() == value
        }),
    )
}

fn apply(image: &mut Image, candidate: Option<&str>) -> u64 {
    let next = candidate.map(str::to_owned);
    let changed = next.is_some() && next != image.stored;
    if changed {
        image.stored = next;
    }
    u64::from(changed)
}

fn recover(export: &Export, preimage: &str) -> Result<u64, Stop> {
    let mut reads = 0_u64;
    let mut ids = Vec::new();
    for input in &export.inputs {
        ids.push(observe_row("export", input, &mut reads)?.semantic_id);
    }
    let mut reread = 0_u64;
    if digest(&ids, &mut reread)? != preimage {
        return Err(Stop::Broken("recovery preimage drifted".to_owned()));
    }
    Ok(reads)
}

fn prove(inputs: &[Row], change: &Row, seen: &[Seen], change_seen: &Seen) -> Result<u64, Stop> {
    let mut proved = 0_u64;
    for (input, before) in inputs.iter().zip(seen.iter()) {
        let again = observe_row("export", input, &mut proved)?;
        if again.semantic_id != before.semantic_id {
            return Err(Stop::Broken("proof identity drifted".to_owned()));
        }
    }
    let again = observe_row("change", change, &mut proved)?;
    if again.semantic_id != change_seen.semantic_id {
        return Err(Stop::Broken("change proof drifted".to_owned()));
    }
    Ok(proved)
}

fn observe_row(label: &str, row: &Row, calls: &mut u64) -> Result<Seen, Stop> {
    *calls = calls.saturating_add(1);
    let snapshot = observe::declared_snapshot(
        label,
        &row.port,
        &row.selector,
        &row.status,
        row.value.as_deref(),
    )
    .map_err(|error| Stop::Broken(error.to_string()))?;
    Ok(Seen {
        status: row.status.clone(),
        semantic_id: semantic_id(&snapshot)?,
    })
}

fn hold_count(rows: &[Seen]) -> u64 {
    rows.iter()
        .filter(|row| matches!(row.status.as_str(), "uncertain" | "timed-out" | "absent"))
        .count() as u64
}

fn admit(export: &Export, change: &Change) -> Option<&'static str> {
    if fixture::is_construction_name(&change.id)
        || observe::is_named_batch(&change.id)
        || seal::is_local_batch(&change.id)
        || service::is_named_mode(&change.id)
    {
        return Some("fixture-change");
    }
    if export.project != change.project {
        return Some("project-mismatch");
    }
    if export
        .packages
        .iter()
        .any(|package| package.origin != "registry")
    {
        return Some("path-origin");
    }
    if export.packages.is_empty() {
        return Some("missing-package");
    }
    None
}

fn record(export: &Export, change: &Change, first: &First) -> Datum {
    let text = canonical(export, change, first);
    let input_ids = first
        .input_ids
        .iter()
        .cloned()
        .map(Datum::String)
        .collect::<Vec<_>>();
    Datum::Node {
        tag: Symbol::qualified("world", "succession-v1"),
        fields: vec![
            (Symbol::new("change"), Datum::String(change.id.clone())),
            (
                Symbol::new("change-id"),
                Datum::String(first.change_id.clone()),
            ),
            (
                Symbol::new("closed"),
                Datum::String(first.closed.to_owned()),
            ),
            (
                Symbol::new("converge-first"),
                query::count_datum(first.converge),
            ),
            (
                Symbol::new("converge-second"),
                query::count_datum(first.converge_second),
            ),
            (
                Symbol::new("deliver-first"),
                query::count_datum(first.deliver),
            ),
            (
                Symbol::new("deliver-second"),
                query::count_datum(first.deliver_second),
            ),
            (Symbol::new("effects"), query::count_datum(first.effects)),
            (Symbol::new("holds"), query::count_datum(first.holds)),
            (Symbol::new("input-ids"), Datum::Vector(input_ids)),
            (Symbol::new("land"), query::count_datum(first.land)),
            (Symbol::new("linux"), Datum::String("absent".to_owned())),
            (Symbol::new("observed"), query::count_datum(first.observed)),
            (
                Symbol::new("origin"),
                Datum::String(export.packages[0].origin.clone()),
            ),
            (
                Symbol::new("postimage"),
                Datum::String(first.postimage.clone()),
            ),
            (
                Symbol::new("preimage"),
                Datum::String(first.preimage.clone()),
            ),
            (
                Symbol::new("project"),
                Datum::String(export.project.clone()),
            ),
            (Symbol::new("proved"), query::count_datum(first.proved)),
            (Symbol::new("record"), Datum::String(text)),
            (
                Symbol::new("recovery"),
                Datum::String(first.recovery.to_owned()),
            ),
            (
                Symbol::new("recovery-reads"),
                query::count_datum(first.recovery_reads),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("seal"), query::count_datum(first.seal)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(first.subprocesses),
            ),
        ],
    }
}

fn canonical(export: &Export, change: &Change, first: &First) -> String {
    [
        format!("change {}", change.id),
        format!("project {}", export.project),
        format!("origin {}", export.packages[0].origin),
        "warm false".to_owned(),
        "fixture-change false".to_owned(),
        format!("observed {}", first.observed),
        format!("proved {}", first.proved),
        format!("seal {}", first.seal),
        format!("converge-first {}", first.converge),
        format!("converge-second {}", first.converge_second),
        format!("land {}", first.land),
        format!("deliver-first {}", first.deliver),
        format!("deliver-second {}", first.deliver_second),
        format!("recovery {}", first.recovery),
        format!("recovery-reads {}", first.recovery_reads),
        format!("holds {}", first.holds),
        "linux absent".to_owned(),
        format!("closed {}", first.closed),
        format!("preimage {}", first.preimage),
        format!("postimage {}", first.postimage),
    ]
    .join("; ")
}

fn digest(ids: &[String], seals: &mut u64) -> Result<String, Stop> {
    *seals = seals.saturating_add(1);
    let mut ordered = ids.to_vec();
    ordered.sort();
    let id = content_id(Datum::String(ordered.join("\n")))
        .map_err(|error| Stop::Broken(error.to_string()))?;
    Ok(hex(&id.bytes))
}

fn semantic_id(snapshot: &Datum) -> Result<String, Stop> {
    let Datum::Node { tag, fields } = snapshot else {
        return Err(Stop::Broken("observation is not a node".to_owned()));
    };
    if tag.as_qualified_str() == "world/observation-gap-v1" {
        return Err(Stop::Gap("observation-gap"));
    }
    if tag.as_qualified_str() != "world/snapshot-v1" {
        return Err(Stop::Broken("observation is not a snapshot".to_owned()));
    }
    let observations = vector_field(fields, "observations")?;
    let Some(Datum::Node { tag, fields }) = observations.first() else {
        return Err(Stop::Broken("snapshot has no observation".to_owned()));
    };
    if tag.as_qualified_str() != "world/observation-v1" {
        return Err(Stop::Broken(
            "snapshot row is not an observation".to_owned(),
        ));
    }
    string_field(fields, "semantic-id")
}

fn parse_export(text: &str) -> Result<Export, Stop> {
    if text.contains('\r') {
        return Err(Stop::Gap("bad-record"));
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut lines = body.lines();
    if lines.next() != Some("export world/succession-export-v1") {
        return Err(Stop::Gap("bad-record"));
    }
    let mut packages = Vec::new();
    let mut project = None;
    let mut inputs = Vec::new();
    for line in lines {
        if line.is_empty() {
            return Err(Stop::Gap("bad-record"));
        }
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("package") => {
                let name = require_token(fields.next())?;
                let version = require_token(fields.next())?;
                let origin = require_token(fields.next())?;
                if fields.next().is_some() || packages.len() == MAX_PACKAGES {
                    return Err(Stop::Gap("bad-record"));
                }
                let _ = (name, version);
                packages.push(Package {
                    origin: origin.to_owned(),
                });
            }
            Some("project") => {
                let name = require_token(fields.next())?;
                if fields.next().is_some() || project.is_some() {
                    return Err(Stop::Gap("bad-record"));
                }
                project = Some(name.to_owned());
            }
            Some("branch") => return Err(Stop::Gap("private-branch")),
            Some("input") => {
                if inputs.len() == MAX_INPUTS {
                    return Err(Stop::Gap("bound"));
                }
                inputs.push(parse_row(&mut fields)?);
            }
            _ => return Err(Stop::Gap("bad-record")),
        }
    }
    let Some(project) = project else {
        return Err(Stop::Gap("bad-record"));
    };
    if packages.is_empty() || inputs.is_empty() {
        return Err(Stop::Gap("bad-record"));
    }
    Ok(Export {
        packages,
        project,
        inputs,
    })
}

fn parse_change(text: &str) -> Result<Change, Stop> {
    if text.contains('\r') || text.contains('\n') {
        return Err(Stop::Gap("bad-record"));
    }
    let mut fields = text.split_whitespace();
    let id = require_token(fields.next())?;
    let project = require_token(fields.next())?;
    let row = parse_row(&mut fields)?;
    if fields.next().is_some() {
        return Err(Stop::Gap("bad-record"));
    }
    Ok(Change {
        id: id.to_owned(),
        project: project.to_owned(),
        row,
    })
}

fn parse_row<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<Row, Stop> {
    let selector = fields.next().ok_or(Stop::Gap("bad-record"))?;
    let status = require_token(fields.next())?;
    let raw_value = fields.next().ok_or(Stop::Gap("bad-record"))?;
    if raw_value != "-" && !token(raw_value) {
        return Err(Stop::Gap("bad-record"));
    }
    if !STATUSES.contains(&status) {
        return Err(Stop::Gap("bad-record"));
    }
    let Some((port, _)) = split_selector(selector) else {
        return Err(Stop::Gap("bad-record"));
    };
    let value = if raw_value == "-" {
        None
    } else {
        Some(raw_value.to_owned())
    };
    if (status == "observed") != value.is_some() {
        return Err(Stop::Gap("bad-record"));
    }
    Ok(Row {
        port: port.to_owned(),
        selector: selector.to_owned(),
        status: status.to_owned(),
        value,
    })
}

fn split_selector(selector: &str) -> Option<(&str, &str)> {
    let (port, rest) = selector.split_once('/')?;
    if token(port) && token(rest) {
        Some((port, rest))
    } else {
        None
    }
}

fn require_token(value: Option<&str>) -> Result<&str, Stop> {
    let value = value.ok_or(Stop::Gap("bad-record"))?;
    if token(value) {
        Ok(value)
    } else {
        Err(Stop::Gap("bad-record"))
    }
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && !value.starts_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '_')
}

fn private_text(text: &str) -> bool {
    PRIVATE.iter().any(|marker| text.contains(marker))
}

fn warm(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("file://")
        || lower.contains(".git")
        || lower.contains("transcript")
        || lower.contains("checkout")
        || text.contains('\\')
        || text.contains("..")
        || text.contains("target/")
        || text.contains("/target")
        || text.contains("cache/")
        || text.contains("/cache")
        || text
            .split_whitespace()
            .any(|word| word.starts_with('/') || word.starts_with('~'))
}

fn claims_linux(text: &str) -> bool {
    text.lines()
        .any(|line| line.split_whitespace().next() == Some("claim"))
}

fn stop_datum(stop: Stop, host: &Host) -> Result<Datum, WorldError> {
    match stop {
        Stop::Gap(reason) => Ok(gap(reason, host)),
        Stop::Broken(detail) => Err(WorldError::Qualification(detail)),
    }
}

fn gap(reason: &str, host: &Host) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "succession-gap-v1"),
        fields: vec![
            (Symbol::new("closed"), Datum::String("open".to_owned())),
            (Symbol::new("effects"), query::count_datum(host.effects)),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
            (Symbol::new("resident"), Datum::Bool(true)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(host.subprocesses),
            ),
        ],
    }
}

fn string_field(fields: &[(Symbol, Datum)], name: &str) -> Result<String, Stop> {
    match field(fields, name)? {
        Datum::String(value) => Ok(value.clone()),
        _ => Err(Stop::Broken(format!("{name} is not text"))),
    }
}

fn vector_field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a [Datum], Stop> {
    match field(fields, name)? {
        Datum::Vector(values) => Ok(values),
        _ => Err(Stop::Broken(format!("{name} is not a vector"))),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a Datum, Stop> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .ok_or_else(|| Stop::Broken(format!("missing {name}")))
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
