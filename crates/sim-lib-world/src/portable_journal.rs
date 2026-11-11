// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Journal replay gate for the portable phase union.
//!
//! The leaf calls the loaded journal command. One replay exports the
//! published journal and reopens it. A corrupt copy stays refused.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/journal-replay-v1";

#[path = "portable_journal_expect.rs"]
mod expect;
#[path = "portable_journal_parse.rs"]
mod parse;

struct Capture {
    value: Datum,
    stable: bool,
    exports: bool,
}

pub(super) struct OwnedReplay {
    case: String,
    reason: String,
    head: String,
    corrupt: String,
    records: String,
}

pub(super) fn journal_leaf() -> Result<ProofLeaf, WorldError> {
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
    let argv = vec!["sim".to_owned(), "journal".to_owned(), "replay".to_owned()];
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
        name: "phase/02".to_owned(),
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
    if let Some(parsed) = parse::parsed_replay(&capture.value) {
        lines.push(expect::replay_line(&parsed));
    } else {
        lines.push("replay missing".to_owned());
    }
    if !capture.stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(parsed) = parse::parsed_replay(&capture.value) {
        claims.insert(expect::replay_claim(&parsed));
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
