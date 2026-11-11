// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, NumberLiteral, Symbol};
use sim_lib_roadmap_runner::{
    ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
    SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
};

use super::*;
use crate::command::WorldCommandLib;

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "deliver-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "delivery gate has no owner command".into(),
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
        "deliver-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_deliver_command_matches_the_authored_delivery() {
    let command = WorldCommandLib::new().expect("command");
    let planned = command.run(&plan_argv("model")).expect("plan");
    let view = view_text(&planned).expect("view");
    let status = command.run(&status_argv("model", &view)).expect("status");
    let leaf = deliver_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("delivery gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/21");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("omit git/branch/main already-true"));
    assert!(present_claims.contains("action git/branch/topic divergent"));
    assert!(present_claims.contains("action ci/sim-sdk uncertain-reading"));
    assert!(present_claims.contains("action mirror/sim-private denied-access"));
    assert!(present_claims.contains("why git/branch/main already-true"));
    assert!(present_claims.contains("why git/branch/topic divergent"));
    assert!(present_claims.contains("why mirror/sim-private denied-access"));
    assert!(present_claims.contains("native git/branch/topic unavailable-receiver"));
    assert!(present_claims.contains("real git/branch/topic unavailable-receiver"));
    assert!(present_claims.contains("gap git/branch/missing selector-absent"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains("private-material:"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("docs/private"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert_eq!(field_count(&planned, "effects"), "0");
    assert_eq!(field_count(&planned, "subprocesses"), "0");
    assert_eq!(
        field_count(&status, "effects"),
        field_count(&status, "dispatches")
    );
    assert_ne!(field_count(&status, "effects"), "0");
    assert_eq!(field_count(&status, "subprocesses"), "0");
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_cause_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_action_cause(&captured.model_plan, "git/branch/topic", "completed");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("delivery gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("git/branch/topic completed"));
    assert!(!present_claims.contains("action git/branch/topic divergent"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn swapped_causes_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_action_cause(
        &captured.model_plan,
        "git/branch/topic",
        "uncertain-reading",
    );
    captured.model_plan = with_action_cause(&captured.model_plan, "ci/sim-sdk", "divergent");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("delivery gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("git/branch/topic uncertain-reading"));
    assert!(actual_deck.contains("ci/sim-sdk divergent"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn a_joined_omission_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_joined_omission(&captured.model_plan);
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("delivery gate is a source-deck predicate");
    };
    assert!(actual_deck.contains(
        "omitted 1|git/branch/main git/direct/v1 git/peeled/v1 git/tag/v1 registry/checksum/sim-kernel"
    ));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn an_echoed_secret_fails_without_copying_it() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_field(
        &captured.model_plan,
        "realization",
        Datum::String("private-material:s3cret-token".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("delivery gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("leaked"));
    assert!(actual_deck.contains("plan hidden"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("private-material:"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

fn with_action_cause(plan: &Datum, selector: &str, cause: &str) -> Datum {
    let Datum::Node { tag, fields } = plan else {
        panic!("plan");
    };
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == "actions" {
                (field.clone(), map_actions(item, selector, cause))
            } else {
                (field.clone(), item.clone())
            }
        })
        .collect();
    Datum::Node {
        tag: tag.clone(),
        fields,
    }
}

fn map_actions(value: &Datum, selector: &str, cause: &str) -> Datum {
    let Datum::Vector(items) = value else {
        panic!("actions");
    };
    Datum::Vector(
        items
            .iter()
            .map(|item| {
                if field_text(item, "selector") == selector {
                    with_field(item, "cause", Datum::String(cause.to_owned()))
                } else {
                    item.clone()
                }
            })
            .collect(),
    )
}

fn with_joined_omission(plan: &Datum) -> Datum {
    let joined = Datum::Node {
        tag: Symbol::qualified("world", "deliver-omission-v1"),
        fields: vec![
            (
                Symbol::new("acknowledgement"),
                Datum::String("omitted".to_owned()),
            ),
            (Symbol::new("cause"), Datum::String("already-true".to_owned())),
            (
                Symbol::new("class"),
                Datum::String("already-true".to_owned()),
            ),
            (Symbol::new("desired"), Datum::String("model-commit-main".to_owned())),
            (Symbol::new("dispatches"), count(0)),
            (Symbol::new("effects"), count(0)),
            (
                Symbol::new("observations"),
                count(2),
            ),
            (Symbol::new("performer-calls"), count(0)),
            (
                Symbol::new("provenance"),
                Datum::String("model/git/branch/main".to_owned()),
            ),
            (
                Symbol::new("selector"),
                Datum::String(
                    "git/branch/main git/direct/v1 git/peeled/v1 git/tag/v1 registry/checksum/sim-kernel"
                        .to_owned(),
                ),
            ),
            (Symbol::new("state"), Datum::String("current".to_owned())),
            (Symbol::new("subprocesses"), count(0)),
        ],
    };
    with_field(plan, "omitted", Datum::Vector(vec![joined]))
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

fn field_text(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::String(text)) => text.clone(),
        _ => String::new(),
    }
}

fn count(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}
