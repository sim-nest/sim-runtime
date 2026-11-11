// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Observation gate for the portable phase union.
//!
//! The leaf calls the loaded snapshot command. It reads the authored rows,
//! compares one changed selector, compares one unchanged batch, and asks a
//! native realization to read. Each snapshot row keeps the reads from that
//! request. A count keeps an empty list distinct from a blank token and
//! from one joined token. The actual deck is that command record. The
//! expected deck is the authored observation. The catalog passes only when
//! the record and that observation agree.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

const ENTRY: &str = "cli/main/world";
const KIND: &str = "world/snapshot-v1";
const ROWS: &str = "\
sim-world-rows-v1
label admitted
envelope model-log
ci ci/run uncertain -
filesystem filesystem/readme observed verified-source
filesystem filesystem/secret denied -
git git/HEAD timed-out -
process process/probe unavailable -
registry registry/index absent -
";
const NATIVE: &str = "\
sim-world-rows-v1
label native
realization native
";
const BEFORE: &str = "\
sim-world-rows-v1
label before
git git/HEAD timed-out -
";
const AFTER: &str = "\
sim-world-rows-v1
label after
git git/HEAD observed abc
";
const EXPECTED_ROWS: &[(&str, &str)] = &[
    ("ci/run", "uncertain"),
    ("filesystem/readme", "observed"),
    ("filesystem/secret", "denied"),
    ("git/HEAD", "timed-out"),
    ("process/probe", "unavailable"),
    ("registry/index", "absent"),
];

struct Capture {
    rows: Vec<Datum>,
    native: Datum,
    changed: Datum,
    quiet: Datum,
    stable: bool,
    exports: bool,
}

pub(super) fn observe_leaf() -> Result<ProofLeaf, WorldError> {
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
    let mut rows = Vec::with_capacity(EXPECTED_ROWS.len());
    let mut stable = true;
    for (selector, _) in EXPECTED_ROWS {
        let (value, same) = call_pair(command, &snapshot_argv(ROWS, selector))?;
        stable &= same;
        rows.push(value);
    }
    let (native, native_same) = call_pair(command, &snapshot_argv(NATIVE, "filesystem/readme"))?;
    let (changed, changed_same) = call_pair(command, &diff_argv(BEFORE, AFTER, "git/HEAD"))?;
    let (quiet, quiet_same) = call_pair(command, &diff_argv(ROWS, ROWS, "filesystem/readme"))?;
    stable &= native_same && changed_same && quiet_same;
    Ok(Capture {
        rows,
        native,
        changed,
        quiet,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/09".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expected_deck(),
        required_claims: required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn snapshot_argv(document: &str, selector: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "snapshot".to_owned(),
        document.to_owned(),
        selector.to_owned(),
    ]
}

fn diff_argv(before: &str, after: &str, selector: &str) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "snapshot-diff".to_owned(),
        before.to_owned(),
        after.to_owned(),
        selector.to_owned(),
    ]
}

fn call_pair(command: &WorldCommandLib, argv: &[String]) -> Result<(Datum, bool), WorldError> {
    let first = command.run(argv)?;
    let second = command.run(argv)?;
    let same = first == second;
    Ok((first, same))
}

fn expected_deck() -> String {
    let mut lines = identity_lines();
    lines.extend(EXPECTED_ROWS.iter().map(|(selector, status)| {
        format!(
            "status {selector} {status} matched {}",
            read_list(&[(*selector).to_owned()])
        )
    }));
    lines.push(
        "native filesystem/readme unmatched unavailable native-capability-missing".to_owned(),
    );
    lines.push("native-reads 0".to_owned());
    lines.push("diff git/HEAD changed timed-out observed".to_owned());
    lines.push(format!(
        "diff-reads {}",
        read_list(&["git/HEAD".to_owned(), "git/HEAD".to_owned()])
    ));
    lines.push("quiet filesystem/readme unchanged".to_owned());
    lines.push("quiet-reads 0".to_owned());
    lines.join("\n")
}

fn identity_lines() -> Vec<String> {
    vec![
        format!("entry {ENTRY}"),
        format!("kind {KIND}"),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
    ]
}

fn actual_deck(capture: &Capture) -> String {
    let entry = if capture.exports { ENTRY } else { "missing" };
    let values = capture_values(capture);
    let (effects, subprocesses, resident) = meter(&values);
    let mut lines = vec![
        format!("entry {entry}"),
        format!("kind {}", shared_kind(&capture.rows, &capture.native)),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    for (index, (selector, _)) in EXPECTED_ROWS.iter().enumerate() {
        let value = capture.rows.get(index);
        lines.push(match value.and_then(parsed_row) {
            Some(row) => format!(
                "status {} {} {} {}",
                row.selector,
                row.status,
                row.mark,
                read_list(&row.reads)
            ),
            None => format!("status {selector} missing"),
        });
    }
    lines.push(native_line(&capture.native));
    lines.push(format!(
        "native-reads {}",
        parsed_row(&capture.native)
            .map(|row| read_list(&row.reads))
            .unwrap_or_else(|| "missing".to_owned())
    ));
    lines.push(diff_line("diff", "git/HEAD", &capture.changed, true));
    lines.push(format!(
        "diff-reads {}",
        parsed_diff(&capture.changed)
            .map(|diff| read_list(&diff.reads))
            .unwrap_or_else(|| "missing".to_owned())
    ));
    lines.push(diff_line(
        "quiet",
        "filesystem/readme",
        &capture.quiet,
        false,
    ));
    lines.push(format!(
        "quiet-reads {}",
        parsed_diff(&capture.quiet)
            .map(|diff| read_list(&diff.reads))
            .unwrap_or_else(|| "missing".to_owned())
    ));
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn capture_values(capture: &Capture) -> Vec<&Datum> {
    let mut values = Vec::new();
    values.extend(capture.rows.iter());
    values.push(&capture.native);
    values.push(&capture.changed);
    values.push(&capture.quiet);
    values
}

fn native_line(value: &Datum) -> String {
    match parsed_row(value) {
        Some(row) => format!(
            "native {} {} {} {}",
            row.selector, row.mark, row.status, row.provenance
        ),
        None => "native missing".to_owned(),
    }
}

fn diff_line(label: &str, selector: &str, value: &Datum, changed: bool) -> String {
    match parsed_diff(value) {
        Some(diff) if diff.changed == changed && changed => format!(
            "{label} {} changed {} {}",
            diff.selector, diff.before_status, diff.after_status
        ),
        Some(diff) if diff.changed == changed => format!("{label} {} unchanged", diff.selector),
        Some(_) => format!("{label} {selector} disagree"),
        None => format!("{label} {selector} missing"),
    }
}

fn required_ids() -> BTreeSet<String> {
    let mut claims = EXPECTED_ROWS
        .iter()
        .map(|(selector, status)| format!("status {selector} {status}"))
        .collect::<BTreeSet<_>>();
    claims.insert("native filesystem/readme unavailable".to_owned());
    claims
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for (index, _) in EXPECTED_ROWS.iter().enumerate() {
        if let Some(row) = capture.rows.get(index).and_then(parsed_row) {
            claims.insert(format!("status {} {}", row.selector, row.status));
        }
    }
    if let Some(row) = parsed_row(&capture.native) {
        claims.insert(format!("native {} {}", row.selector, row.status));
    }
    claims
}

struct Row {
    selector: String,
    status: String,
    mark: &'static str,
    provenance: String,
    reads: Vec<String>,
}

struct DiffRow {
    selector: String,
    changed: bool,
    before_status: String,
    after_status: String,
    reads: Vec<String>,
}

fn parsed_row(value: &Datum) -> Option<Row> {
    if kind_of(value) != KIND {
        return None;
    }
    let observation = first_observation(value)?;
    let selector = field_text(value, "selector");
    let status = field_text(&observation, "status");
    if selector == "missing" || status == "missing" {
        return None;
    }
    let mark = match field(&observation, "matched") {
        Some(Datum::Bool(true)) => "matched",
        Some(Datum::Bool(false)) => "unmatched",
        _ => return None,
    };
    Some(Row {
        selector,
        status,
        mark,
        provenance: field_text(&observation, "provenance"),
        reads: string_list(field(value, "reads"))?,
    })
}

fn parsed_diff(value: &Datum) -> Option<DiffRow> {
    if kind_of(value) != "world/observation-diff-v1" {
        return None;
    }
    let changed = match field(value, "changed") {
        Some(Datum::Bool(value)) => *value,
        _ => return None,
    };
    let selector = field_text(value, "selector");
    if selector == "missing" {
        return None;
    }
    Some(DiffRow {
        selector,
        changed,
        before_status: field_text(value, "before-status"),
        after_status: field_text(value, "after-status"),
        reads: string_list(field(value, "reads"))?,
    })
}

fn first_observation(value: &Datum) -> Option<Datum> {
    let Datum::Vector(items) = field(value, "observations")? else {
        return None;
    };
    items.first().cloned()
}

fn shared_kind(rows: &[Datum], native: &Datum) -> String {
    let mut kinds = rows.iter().map(kind_of).collect::<BTreeSet<_>>();
    kinds.insert(kind_of(native));
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

fn read_list(reads: &[String]) -> String {
    if reads.is_empty() {
        "0".to_owned()
    } else {
        format!("{}|{}", reads.len(), reads.join("|"))
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
#[path = "portable_observe_tests.rs"]
mod tests;
