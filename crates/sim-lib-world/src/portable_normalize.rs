// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Identity normalization gate for the portable phase union.
//!
//! The leaf calls the loaded bind command. One normalize admits a subject
//! through the kernel content id. A field or role change moves that id.
//! An ordering alias or a diagnostic change does not.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/bind-normalize-v1";

#[path = "portable_normalize_expect.rs"]
mod expect;
#[path = "portable_normalize_parse.rs"]
mod parse;

struct Capture {
    value: Datum,
    stable: bool,
    exports: bool,
}

pub(super) struct OwnedNormalize {
    case: String,
    reason: String,
    alias: String,
    diagnostic: String,
    field: String,
    role: String,
    wrong_role: String,
    old_byte: String,
    authority: String,
}

pub(super) fn normalize_leaf() -> Result<ProofLeaf, WorldError> {
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
    let argv = vec!["sim".to_owned(), "bind".to_owned(), "normalize".to_owned()];
    let first = command.run(&argv)?;
    let second = command.run(&argv)?;
    let stable = first == second;
    Ok(Capture {
        value: first,
        stable,
        exports,
    })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/03".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn actual_deck(capture: &Capture) -> String {
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {}", field_count(&capture.value, "effects")),
        format!(
            "subprocesses {}",
            field_count(&capture.value, "subprocesses")
        ),
        format!("resident {}", field_bool(&capture.value, "resident")),
    ];
    if let Some(parsed) = parse::parsed_normalize(&capture.value) {
        lines.push(expect::normalize_line(&parsed));
    } else {
        lines.push("normalize missing".to_owned());
    }
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(parsed) = parse::parsed_normalize(&capture.value) {
        claims.insert(expect::normalize_claim(&parsed));
    }
    claims
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

pub(super) fn kind_of(value: &Datum) -> String {
    match value {
        Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}
