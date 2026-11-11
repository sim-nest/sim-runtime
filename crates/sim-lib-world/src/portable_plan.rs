// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Plan gate for the portable phase union.
//!
//! The leaf calls the loaded world command. Each build runs the expression
//! twice and also asks for the text and card views. The actual deck is that
//! command record. The expected deck is the authored plan for the bundled
//! subject. The catalog passes only when the record and that plan agree.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;

const KIND: &str = "world/proof-plan-v1";
const ENTRY: &str = "cli/main/world";
const LEGAL_READY: &str = "conclusion/ready-b conclusion/ready-a";
const FLOOR: &str = "conclusion/gap-source conclusion/ready-a conclusion/ready-b conclusion/revoked conclusion/stranger";
const REASONS: &[&str] = &[
    "reason conclusion/gap-source excluded missing-source",
    "reason conclusion/ready-a included legal-ready",
    "reason conclusion/ready-b included legal-ready",
    "reason conclusion/revoked excluded revoked-evidence",
    "reason conclusion/stranger excluded unknown-owner",
];

pub(super) fn plan_leaf() -> Result<ProofLeaf, WorldError> {
    let command = super::project::resident_command()?;
    let exports = command.manifest().exports.iter().any(|export| {
        matches!(
            export,
            Export::Function { symbol, .. } if symbol.to_string() == ENTRY
        )
    });
    let expression = plan_argv(None);
    let first = command.run(&expression)?;
    let second = command.run(&expression)?;
    let text = command.run(&plan_argv(Some("text")))?;
    let card = command.run(&plan_argv(Some("card")))?;
    Ok(leaf_from(
        exports,
        &first,
        first == second,
        views_agree(&first, &text, &card),
    ))
}

fn plan_argv(view: Option<&str>) -> Vec<String> {
    let mut argv = vec!["sim".to_owned(), "plan".to_owned()];
    if let Some(view) = view {
        argv.push(view.to_owned());
    }
    argv
}

pub(super) fn leaf_from(
    exports: bool,
    value: &Datum,
    stable: bool,
    views_agree: bool,
) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/08".to_owned(),
        actual_deck: actual_deck(exports, value, stable, views_agree),
        expected_deck: expected_deck(),
        required_claims: required_ids(),
        present_claims: present_reasons(value),
    }
}

fn required_ids() -> BTreeSet<String> {
    BTreeSet::from([
        "legal-ready".to_owned(),
        "missing-source".to_owned(),
        "revoked-evidence".to_owned(),
        "unknown-owner".to_owned(),
    ])
}

fn expected_deck() -> String {
    let mut lines = identity_lines(ENTRY);
    lines.extend(REASONS.iter().map(|line| (*line).to_owned()));
    lines.push("views agree".to_owned());
    lines.join("\n")
}

fn actual_deck(exports: bool, value: &Datum, stable: bool, views_agree: bool) -> String {
    let entry = if exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("kind {}", kind_of(value)),
        format!("page {}", field_count(value, "page")),
        format!("effects {}", field_count(value, "effects")),
        format!("subprocesses {}", field_count(value, "subprocesses")),
        format!("resident {}", field_bool(value, "resident")),
        format!("world {}", world_mark(value)),
        labeled("affected", &joined(value, "affected")),
        labeled("legal-ready", &joined(value, "legal-ready")),
        labeled("floor", &joined(value, "floor")),
    ];
    lines.extend(reason_lines(value));
    lines.push(if views_agree {
        "views agree".to_owned()
    } else {
        "views differ".to_owned()
    });
    if !stable {
        lines.push("unstable".to_owned());
    }
    lines.join("\n")
}

fn labeled(label: &str, value: &str) -> String {
    if value.is_empty() {
        label.to_owned()
    } else {
        format!("{label} {value}")
    }
}

fn identity_lines(entry: &str) -> Vec<String> {
    vec![
        format!("entry {entry}"),
        format!("kind {KIND}"),
        "page 0".to_owned(),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        "world sealed".to_owned(),
        "affected".to_owned(),
        format!("legal-ready {LEGAL_READY}"),
        format!("floor {FLOOR}"),
    ]
}

fn kind_of(value: &Datum) -> String {
    match value {
        Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}

fn world_mark(value: &Datum) -> String {
    match field_string(value, "world") {
        text if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            "sealed".to_owned()
        }
        _ => "missing".to_owned(),
    }
}

fn views_agree(expression: &Datum, text: &Datum, card: &Datum) -> bool {
    let world = field_text(expression, "world");
    world.len() == 64
        && world.bytes().all(|byte| byte.is_ascii_hexdigit())
        && field_text(text, "world") == world
        && field_text(card, "core/subject") == world
        && field_text(card, "core/kind") == KIND
        && joined(expression, "legal-ready") == field_text(text, "legal-ready")
        && field_text(text, "legal-ready") == field_text(card, "core/result")
        && joined(expression, "floor") == field_text(text, "floor")
        && joined(expression, "affected") == field_text(text, "affected")
        && field_count(expression, "effects") == field_count(text, "effects")
        && field_count(expression, "subprocesses") == field_count(text, "subprocesses")
        && field_bool(expression, "resident") == field_bool(text, "resident")
}

fn reason_lines(value: &Datum) -> Vec<String> {
    let Some(Datum::Vector(items)) = field(value, "leaves") else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for item in items {
        let disposition = match field(item, "included") {
            Some(Datum::Bool(true)) => "included",
            Some(Datum::Bool(false)) => "excluded",
            _ => "missing",
        };
        lines.push(format!(
            "reason {} {disposition} {}",
            field_string(item, "conclusion"),
            field_string(item, "reason")
        ));
    }
    lines.sort();
    lines
}

fn present_reasons(value: &Datum) -> BTreeSet<String> {
    let Some(Datum::Vector(items)) = field(value, "leaves") else {
        return BTreeSet::new();
    };
    items
        .iter()
        .filter_map(|item| match field_string(item, "reason") {
            reason if reason != "missing" && !reason.is_empty() => Some(reason),
            _ => None,
        })
        .collect()
}

fn joined(value: &Datum, name: &str) -> String {
    let Some(Datum::Vector(items)) = field(value, name) else {
        return "missing".to_owned();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Datum::String(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
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

fn field_string(value: &Datum, name: &str) -> String {
    match field_text(value, name) {
        text if !text.is_empty() && text != "missing" => text,
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
mod tests {
    use sim_kernel::{Datum, Symbol};
    use sim_lib_roadmap_runner::{
        ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
        SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
    };

    use super::*;
    use crate::command::WorldCommandLib;

    struct RefuseLaunch;

    impl SandboxLauncher for RefuseLaunch {
        fn id(&self) -> &str {
            "plan-gate-refuse"
        }

        fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            SandboxAttempt::Refused(SandboxRefusal {
                launcher: self.id().into(),
                reason: "plan gate has no owner command".into(),
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
            "plan-gate",
        )
        .expect("leaf");
        receipt.disposition
    }

    #[test]
    fn the_loaded_plan_command_matches_the_authored_plan() {
        let command = WorldCommandLib::new().expect("command");
        let recorded = command.run(&plan_argv(None)).expect("plan");
        let text = command.run(&plan_argv(Some("text"))).expect("text");
        let card = command.run(&plan_argv(Some("card"))).expect("card");
        let leaf = plan_leaf().expect("leaf");
        let ProofLeaf::SourceDeckPredicate {
            actual_deck,
            expected_deck,
            present_claims,
            required_claims,
            ..
        } = &leaf
        else {
            panic!("plan gate is a source-deck predicate");
        };
        assert_eq!(actual_deck, expected_deck);
        assert_eq!(present_claims, required_claims);
        assert_eq!(present_claims, &present_reasons(&recorded));
        assert_eq!(field_count(&recorded, "effects"), "0");
        assert_eq!(field_count(&recorded, "subprocesses"), "0");
        assert!(views_agree(&recorded, &text, &card));
        assert_eq!(disposition(leaf), ProofDisposition::Passed);
    }

    #[test]
    fn a_record_that_drops_a_reason_fails() {
        let value = Datum::Node {
            tag: Symbol::qualified("world", "proof-plan-v1"),
            fields: vec![
                (Symbol::new("world"), Datum::String("a".repeat(64))),
                (Symbol::new("page"), crate::query::count_datum(0)),
                (Symbol::new("effects"), crate::query::count_datum(0)),
                (Symbol::new("subprocesses"), crate::query::count_datum(0)),
                (Symbol::new("resident"), Datum::Bool(true)),
                (Symbol::new("affected"), Datum::Vector(Vec::new())),
                (
                    Symbol::new("legal-ready"),
                    Datum::Vector(
                        LEGAL_READY
                            .split(' ')
                            .map(|text| Datum::String(text.to_owned()))
                            .collect(),
                    ),
                ),
                (
                    Symbol::new("floor"),
                    Datum::Vector(
                        FLOOR
                            .split(' ')
                            .map(|text| Datum::String(text.to_owned()))
                            .collect(),
                    ),
                ),
                (Symbol::new("leaves"), Datum::Vector(Vec::new())),
            ],
        };
        let leaf = leaf_from(true, &value, true, true);
        let ProofLeaf::SourceDeckPredicate {
            actual_deck,
            expected_deck,
            present_claims,
            ..
        } = &leaf
        else {
            panic!("plan gate is a source-deck predicate");
        };
        assert_ne!(actual_deck, expected_deck);
        assert!(present_claims.is_empty());
        assert_eq!(disposition(leaf), ProofDisposition::Failed);
    }
}
