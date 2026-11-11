// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Canary gate for the portable phase union.
//!
//! The leaf calls the loaded canary command. One fresh model branch carries
//! the validated source head, one dispatch, and a zero-work repeat. A wrong
//! or consumed purpose grant refuses. A public branch, tag, package, or
//! merge is refused. A private value is not copied. Native and real
//! realizations do not carry the model reading.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/canary-v1";
const GAP: &str = "world/canary-gap-v1";
const SOURCE: &str = "world/canary-source-v1";
const BRANCH: &str = "world/canary-branch-v1";

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

const PURPOSE: &str = "world/model-canary";
const SOURCE_SELECTOR: &str = "git/branch/main";
const FRESH: &str = "git/branch/canary-1";

#[path = "portable_canary_expect.rs"]
mod expect;
#[path = "portable_canary_parse.rs"]
mod parse;

struct Capture {
    fresh: Datum,
    wrong: Datum,
    spent: Datum,
    branch: Datum,
    tag: Datum,
    package: Datum,
    merge: Datum,
    private_purpose: Datum,
    native: Datum,
    real: Datum,
    stable: bool,
    exports: bool,
}

pub(super) struct OwnedCanary {
    realization: String,
    class: String,
    source_selector: String,
    source_kind: String,
    source_state: String,
    source_provenance: String,
    source_value: String,
    destination_selector: String,
    destination_state: String,
    destination_provenance: String,
    destination_value: String,
    dispatches: String,
    repeat_dispatches: String,
    performer_calls: String,
    repeat_performer_calls: String,
    observations: String,
    grant_purpose: String,
    grant_state: String,
    disclosure: String,
    recovered: String,
    resident: String,
    effects: String,
    subprocesses: String,
}

pub(super) struct OwnedGap {
    realization: String,
    reason: String,
    source: String,
    destination: String,
    purpose: String,
    dispatches: String,
}

pub(super) fn canary_leaf() -> Result<ProofLeaf, WorldError> {
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
    let (fresh, fresh_same) = call_pair(
        command,
        &argv("model", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"),
    )?;
    let (wrong, wrong_same) = call_pair(
        command,
        &argv(
            "model",
            SOURCE_SELECTOR,
            "git/branch/canary-3",
            "world/deliver",
            "issued",
        ),
    )?;
    let (spent, spent_same) = call_pair(
        command,
        &argv(
            "model",
            SOURCE_SELECTOR,
            "git/branch/canary-3",
            PURPOSE,
            "spent",
        ),
    )?;
    let (branch, branch_same) = call_pair(
        command,
        &argv("model", SOURCE_SELECTOR, SOURCE_SELECTOR, PURPOSE, "issued"),
    )?;
    let (tag, tag_same) = call_pair(
        command,
        &argv("model", SOURCE_SELECTOR, "git/tag/v1", PURPOSE, "issued"),
    )?;
    let (package, package_same) = call_pair(
        command,
        &argv(
            "model",
            SOURCE_SELECTOR,
            "registry/checksum/sim-kernel",
            PURPOSE,
            "issued",
        ),
    )?;
    let (merge, merge_same) = call_pair(
        command,
        &argv(
            "model",
            SOURCE_SELECTOR,
            "git/merge/topic",
            PURPOSE,
            "issued",
        ),
    )?;
    let (private_purpose, private_same) = call_pair(
        command,
        &argv(
            "model",
            SOURCE_SELECTOR,
            "git/branch/canary-5",
            "private-material:s3cret-token",
            "issued",
        ),
    )?;
    let (native, native_same) = call_pair(
        command,
        &argv("native", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"),
    )?;
    let (real, real_same) = call_pair(
        command,
        &argv("real", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"),
    )?;
    let stable = fresh_same
        && wrong_same
        && spent_same
        && branch_same
        && tag_same
        && package_same
        && merge_same
        && private_same
        && native_same
        && real_same;
    Ok(Capture {
        fresh,
        wrong,
        spent,
        branch,
        tag,
        package,
        merge,
        private_purpose,
        native,
        real,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/22".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn argv(
    realization: &str,
    source: &str,
    destination: &str,
    purpose: &str,
    presentation: &str,
) -> Vec<String> {
    vec![
        "sim".to_owned(),
        "canary".to_owned(),
        realization.to_owned(),
        source.to_owned(),
        destination.to_owned(),
        purpose.to_owned(),
        presentation.to_owned(),
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
    let (effects, subprocesses, resident) = meter(capture);
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {effects}"),
        format!("subprocesses {subprocesses}"),
        format!("resident {resident}"),
    ];
    push_canary(&mut lines, &capture.fresh);
    push_gap(&mut lines, "wrong", &capture.wrong);
    push_gap(&mut lines, "spent", &capture.spent);
    push_gap(&mut lines, "branch", &capture.branch);
    push_gap(&mut lines, "tag", &capture.tag);
    push_gap(&mut lines, "package", &capture.package);
    push_gap(&mut lines, "merge", &capture.merge);
    push_gap(&mut lines, "private", &capture.private_purpose);
    push_gap(&mut lines, "native", &capture.native);
    push_gap(&mut lines, "real", &capture.real);
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    if values.iter().any(|value| contains_private(value)) {
        lines.push("leaked".to_owned());
    }
    lines.join("\n")
}

fn push_canary(lines: &mut Vec<String>, value: &Datum) {
    let Some(row) = parse::parsed_canary(value) else {
        lines.push("canary missing".to_owned());
        return;
    };
    lines.push(expect::canary_line(&row));
}

fn push_gap(lines: &mut Vec<String>, label: &str, value: &Datum) {
    let Some(row) = parse::parsed_gap(value) else {
        lines.push(format!("gap {label} missing"));
        return;
    };
    lines.push(expect::gap_line(label, &row));
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(row) = parse::parsed_canary(&capture.fresh) {
        claims.insert(format!(
            "canary {} {} dispatches {} repeat {}",
            row.destination_selector, row.class, row.dispatches, row.repeat_dispatches
        ));
    }
    for (label, value) in [
        ("wrong", &capture.wrong),
        ("spent", &capture.spent),
        ("branch", &capture.branch),
        ("tag", &capture.tag),
        ("package", &capture.package),
        ("merge", &capture.merge),
        ("private", &capture.private_purpose),
        ("native", &capture.native),
        ("real", &capture.real),
    ] {
        if let Some(row) = parse::parsed_gap(value) {
            claims.insert(format!("gap {label} {}", row.reason));
        }
    }
    claims
}

fn capture_values(capture: &Capture) -> Vec<&Datum> {
    vec![
        &capture.fresh,
        &capture.wrong,
        &capture.spent,
        &capture.branch,
        &capture.tag,
        &capture.package,
        &capture.merge,
        &capture.private_purpose,
        &capture.native,
        &capture.real,
    ]
}

fn meter(capture: &Capture) -> (String, String, String) {
    let mut effects = "0".to_owned();
    let mut subprocesses = "0".to_owned();
    let mut resident = "true".to_owned();
    for value in capture_values(capture) {
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
#[path = "portable_canary_tests.rs"]
mod tests;
