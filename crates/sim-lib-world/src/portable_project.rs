// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project gate for the portable phase union.
//!
//! The leaf calls the loaded world command. That bundled command is retained
//! for the process, and each build of the leaf runs it twice. The actual deck
//! is that command record. The expected deck is the authored consumer set for
//! the bundled source fact. The catalog passes only when the record and that
//! set agree.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;
use crate::product::{DISCLOSURE_CONCLUSION, SOURCE_CONCLUSION, SOURCE_FACT};
use crate::provider::config_symbol;

const KIND: &str = "world/public-api-v1";
const ENTRY: &str = "cli/main/world";
const INPUT: &str = "pub fn project()";

pub(super) fn project_leaf() -> Result<ProofLeaf, WorldError> {
    let command = resident_command()?;
    let exports = command.manifest().exports.iter().any(|export| {
        matches!(
            export,
            Export::Function { symbol, .. } if symbol.to_string() == ENTRY
        )
    });
    let argv = project_argv();
    let first = command.run(&argv)?;
    let second = command.run(&argv)?;
    Ok(leaf_from(exports, &first, first == second))
}

/// The bundled command has no caller input. One retained copy matches the
/// loaded entrypoint, which keeps its product and dispatches each call.
pub(crate) fn resident_command() -> Result<WorldCommandLib, WorldError> {
    static RESIDENT: OnceLock<WorldCommandLib> = OnceLock::new();
    if let Some(command) = RESIDENT.get() {
        return Ok(command.clone());
    }
    let built = WorldCommandLib::new()?;
    Ok(RESIDENT.get_or_init(|| built.clone()).clone())
}

fn project_argv() -> [String; 5] {
    [
        "sim".to_owned(),
        "project".to_owned(),
        KIND.to_owned(),
        SOURCE_FACT.to_owned(),
        INPUT.to_owned(),
    ]
}

pub(super) fn leaf_from(exports: bool, value: &Datum, stable: bool) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/06".to_owned(),
        actual_deck: actual_deck(exports, value, stable),
        expected_deck: expected_deck(),
        required_claims: required_ids(),
        present_claims: affected_ids(value),
    }
}

fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([
        DISCLOSURE_CONCLUSION.to_owned(),
        SOURCE_CONCLUSION.to_owned(),
    ])
}

fn expected_deck() -> String {
    let mut lines = identity_lines(ENTRY);
    for id in required_ids() {
        lines.push(format!("affected {id}"));
    }
    lines.join("\n")
}

fn actual_deck(exports: bool, value: &Datum, stable: bool) -> String {
    let entry = if exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("kind {}", field_string(value, "kind")),
        format!("fact {}", field_string(value, "fact")),
        format!("shape {}", field_string(value, "shape")),
    ];
    for id in affected_ids(value) {
        lines.push(format!("affected {id}"));
    }
    if !stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn identity_lines(entry: &str) -> Vec<String> {
    vec![
        format!("entry {entry}"),
        format!("kind {KIND}"),
        format!("fact {SOURCE_FACT}"),
        format!("shape {}", config_symbol().as_qualified_str()),
    ]
}

fn field_string(value: &Datum, name: &str) -> String {
    let Datum::Node { fields, .. } = value else {
        return "missing".to_owned();
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .and_then(|(_, field)| match field {
            Datum::String(text) if !text.is_empty() => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "missing".to_owned())
}

fn affected_ids(value: &Datum) -> BTreeSet<String> {
    let Datum::Node { fields, .. } = value else {
        return BTreeSet::new();
    };
    let Some((_, field)) = fields
        .iter()
        .find(|(name, _)| name.to_string() == "affected")
    else {
        return BTreeSet::new();
    };
    let Datum::Vector(items) = field else {
        return BTreeSet::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Datum::String(text) if !text.is_empty() => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sim_kernel::{Datum, Symbol};
    use sim_lib_roadmap_runner::{
        ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
        SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
    };

    use super::*;

    struct RefuseLaunch;

    impl SandboxLauncher for RefuseLaunch {
        fn id(&self) -> &str {
            "project-gate-refuse"
        }

        fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            SandboxAttempt::Refused(SandboxRefusal {
                launcher: self.id().into(),
                reason: "project gate has no owner command".into(),
                report: None,
            })
        }
    }

    fn disposition(leaf: ProofLeaf) -> ProofDisposition {
        let name = leaf.name().to_owned();
        let catalog = ProofCatalog::new([leaf]).expect("catalog");
        let receipt = execute_proof_leaf(
            &catalog,
            &name,
            &RefuseLaunch,
            &ProcessCancellation::default(),
            "project-gate",
        )
        .expect("leaf");
        receipt.disposition
    }

    #[test]
    fn the_loaded_project_command_matches_the_authored_consumers() {
        let command = WorldCommandLib::new().expect("command");
        let recorded = command.run(&project_argv()).expect("project");
        let leaf = project_leaf().expect("leaf");
        let ProofLeaf::SourceDeckPredicate {
            actual_deck,
            expected_deck,
            present_claims,
            required_claims,
            ..
        } = &leaf
        else {
            panic!("project gate is a source-deck predicate");
        };
        assert_eq!(actual_deck, expected_deck);
        assert_eq!(present_claims, required_claims);
        assert!(present_claims.contains(SOURCE_CONCLUSION));
        assert!(present_claims.contains(DISCLOSURE_CONCLUSION));
        assert_eq!(present_claims, &affected_ids(&recorded));
        assert_eq!(disposition(leaf), ProofDisposition::Passed);
    }

    #[test]
    fn a_record_that_drops_a_consumer_fails() {
        let value = Datum::Node {
            tag: Symbol::qualified("world", "projection-v1"),
            fields: vec![
                (Symbol::new("kind"), Datum::String(KIND.to_owned())),
                (Symbol::new("fact"), Datum::String(SOURCE_FACT.to_owned())),
                (
                    Symbol::new("shape"),
                    Datum::String(config_symbol().as_qualified_str()),
                ),
                (Symbol::new("affected"), Datum::Vector(Vec::new())),
            ],
        };
        let leaf = leaf_from(true, &value, true);
        let ProofLeaf::SourceDeckPredicate {
            actual_deck,
            expected_deck,
            present_claims,
            ..
        } = &leaf
        else {
            panic!("project gate is a source-deck predicate");
        };
        assert_ne!(actual_deck, expected_deck);
        assert!(present_claims.is_empty());
        assert_eq!(disposition(leaf), ProofDisposition::Failed);
    }
}
