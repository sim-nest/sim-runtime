// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Foreign-project gate for the portable phase union.
//!
//! The leaf calls the loaded succeed command. The specimen and a second
//! project of the same public shape both close. A private branch stays
//! a gap.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/succession-v1";
const GAP: &str = "world/succession-gap-v1";
const SECOND: &str = "harbor";

#[path = "portable_foreign_expect.rs"]
mod expect;
#[path = "portable_foreign_parse.rs"]
mod parse;

struct Capture {
    specimen: Datum,
    second: Datum,
    branch: Datum,
    exports: bool,
}

pub(super) struct OwnedForeign {
    specimen: String,
    second: String,
    branch: String,
}

pub(super) fn foreign_leaf() -> Result<ProofLeaf, WorldError> {
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
    let specimen = call(command, "lantern", false)?;
    let second = call(command, SECOND, false)?;
    let branch = call(command, "lantern", true)?;
    Ok(Capture {
        specimen,
        second,
        branch,
        exports,
    })
}

fn call(command: &WorldCommandLib, project: &str, branch: bool) -> Result<Datum, WorldError> {
    let export = export_text(project, branch);
    let change = format!("shade-1 {project} provider/shade observed shade-next");
    command.run(&["sim".to_owned(), "succeed".to_owned(), export, change])
}

fn export_text(project: &str, branch: bool) -> String {
    let mut text = format!(
        "\
export world/succession-export-v1
package sim-incremental-core 0.5.2 registry
package sim-storage-port 0.2.2 registry
package sim-lib-world 0.1.1 registry
project {project}
input provider/widget observed widget-body
input remote/hostile unavailable -
"
    );
    if branch {
        text.push_str("branch private\n");
    }
    text
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/11".to_owned(),
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
        format!("effects {}", field_count(&capture.specimen, "effects")),
        format!(
            "subprocesses {}",
            field_count(&capture.specimen, "subprocesses")
        ),
        format!("resident {}", field_bool(&capture.specimen, "resident")),
    ];
    if let Some(parsed) = parse::parsed_foreign(capture) {
        lines.push(expect::foreign_line(&parsed));
    } else {
        lines.push("foreign missing".to_owned());
    }
    lines.join("\n")
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if let Some(parsed) = parse::parsed_foreign(capture) {
        claims.insert(expect::foreign_claim(&parsed));
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

pub(super) fn outcome(value: &Datum) -> Option<String> {
    match kind_of(value).as_str() {
        RECORD => text(value, "closed"),
        GAP => text(value, "reason"),
        _ => None,
    }
}

pub(super) fn project_of(value: &Datum) -> Option<String> {
    if kind_of(value) != RECORD {
        return None;
    }
    text(value, "project")
}

fn kind_of(value: &Datum) -> String {
    match value {
        Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
        _ => None,
    }
}
