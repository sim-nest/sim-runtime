// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

#[test]
fn contradictory_text_or_card_views_fail() {
    let command = WorldCommandLib::new().expect("command");
    let expression = command.run(&plan_argv(None)).expect("plan");
    let text = command.run(&plan_argv(Some("text"))).expect("text");
    let card = command.run(&plan_argv(Some("card"))).expect("card");
    assert!(views_agree(&expression, &text, &card));
    let dropped = with_field(&text, "leaves", Datum::String(String::new()));
    assert!(!views_agree(&expression, &dropped, &card));
    let stale = with_field(&card, "core/args", Datum::String("stale".to_owned()));
    assert!(!views_agree(&expression, &text, &stale));
    let bare = with_field(&card, "core/requires", Datum::String(String::new()));
    assert!(!views_agree(&expression, &text, &bare));
    let shifted = with_field(&text, "page", crate::query::count_datum(9));
    assert!(!views_agree(&expression, &shifted, &card));
    let leaf = leaf_from(
        true,
        &expression,
        true,
        views_agree(&expression, &dropped, &card),
    );
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        expected_deck,
        ..
    } = &leaf
    else {
        panic!("plan gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("views differ"));
    assert_ne!(actual_deck, expected_deck);
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

fn with_field(value: &Datum, name: &str, replacement: Datum) -> Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("node");
    };
    let mut found = false;
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == name {
                found = true;
                (field.clone(), replacement.clone())
            } else {
                (field.clone(), item.clone())
            }
        })
        .collect();
    assert!(found, "missing {name}");
    Datum::Node {
        tag: tag.clone(),
        fields,
    }
}
