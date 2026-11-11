// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Evidence gate for the portable phase union.
//!
//! The leaf calls the loaded why command. Grade promotion keeps one key.
//! A wrong domain, revoked support, a missing object, and a storage id
//! used as a semantic key refuse. Telemetry leaves that key unchanged.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/evidence-v1";

#[path = "portable_evidence_expect.rs"]
mod expect;
#[path = "portable_evidence_parse.rs"]
mod parse;

struct Capture {
    value: Datum,
    exports: bool,
}

pub(super) struct OwnedEvidence {
    key: String,
    receipt: String,
    telemetry: String,
    domain: String,
    revoked: String,
    missing: String,
    storage: String,
}

pub(super) fn evidence_leaf() -> Result<ProofLeaf, WorldError> {
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
    let value = command.run(&[
        "sim".to_owned(),
        "why".to_owned(),
        "evidence".to_owned(),
        "admit".to_owned(),
    ])?;
    Ok(Capture { value, exports })
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/12".to_owned(),
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
    if let Some(parsed) = parse::parsed_evidence(capture) {
        lines.push(expect::evidence_line(&parsed));
    } else {
        lines.push("evidence missing".to_owned());
    }
    lines.join("\n")
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(parsed) = parse::parsed_evidence(capture) {
        claims.insert(expect::evidence_claim(&parsed));
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

fn parsed_kind(value: &Datum) -> bool {
    matches!(value, Datum::Node { tag, .. } if tag.as_qualified_str() == RECORD)
}

pub(super) fn text_field(value: &Datum, name: &str) -> Option<String> {
    if !parsed_kind(value) {
        return None;
    }
    match field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
        _ => None,
    }
}
