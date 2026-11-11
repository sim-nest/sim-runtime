// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;
use sim_lib_roadmap_runner::{
    ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
    SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
};

use super::*;
use crate::command::WorldCommandLib;

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "release-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "release gate has no owner command".into(),
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
        "release-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_release_command_matches_the_authored_release() {
    let command = WorldCommandLib::new().expect("command");
    let planned = command.run(&plan_argv("model")).expect("plan");
    let status = command.run(&status_argv("model")).expect("status");
    let leaf = release_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("release gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/20");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("omit git/branch/main"));
    assert!(present_claims.contains("action git/branch/topic desired-differs"));
    assert!(present_claims.contains("why ci/sim-sdk original-preserved"));
    assert!(present_claims.contains("native git/branch/topic original-preserved"));
    assert!(present_claims.contains("real git/branch/topic original-preserved"));
    assert!(present_claims.contains("gap git/branch/missing selector-absent"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains("private-material:"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("docs/private"));
    assert!(!actual_deck.contains("leaked"));
    assert_eq!(field_count(&planned, "effects"), "0");
    assert_eq!(field_count(&planned, "subprocesses"), "0");
    assert_eq!(field_count(&status, "effects"), "0");
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_reason_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_action_reason(&captured.model_plan, "git/branch/topic", "completed");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("release gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("act model git/branch/topic divergent completed"));
    assert!(!present_claims.contains("action git/branch/topic desired-differs"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn swapped_reasons_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_action_reason(
        &captured.model_plan,
        "git/branch/topic",
        "original-preserved",
    );
    captured.model_plan =
        with_action_reason(&captured.model_plan, "ci/sim-kernel", "desired-differs");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("release gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("act model git/branch/topic divergent original-preserved"));
    assert!(actual_deck.contains("act model ci/sim-kernel unknown desired-differs"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn a_joined_omission_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.model_plan = with_joined_omission(&captured.model_plan);
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("release gate is a source-deck predicate");
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
        "disclosure",
        Datum::String("private-material:s3cret-token".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("release gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("leaked"));
    assert!(actual_deck.contains("disclosure hidden"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("private-material:"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

fn with_action_reason(plan: &Datum, selector: &str, reason: &str) -> Datum {
    let Datum::Node { tag, fields } = plan else {
        panic!("plan");
    };
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == "actions" {
                (field.clone(), map_actions(item, selector, reason))
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

fn map_actions(value: &Datum, selector: &str, reason: &str) -> Datum {
    let Datum::Vector(items) = value else {
        panic!("actions");
    };
    Datum::Vector(
        items
            .iter()
            .map(|item| {
                if field_text(item, "selector") == selector {
                    with_field(item, "reason", Datum::String(reason.to_owned()))
                } else {
                    item.clone()
                }
            })
            .collect(),
    )
}

fn with_joined_omission(plan: &Datum) -> Datum {
    let joined = Datum::Node {
        tag: sim_kernel::Symbol::qualified("world", "release-omission-v1"),
        fields: vec![
            (
                sim_kernel::Symbol::new("disclosure"),
                Datum::String("policy/no-v3-disclosure".to_owned()),
            ),
            (
                sim_kernel::Symbol::new("intent"),
                Datum::String("intent/reconcile/joined".to_owned()),
            ),
            (
                sim_kernel::Symbol::new("reason"),
                Datum::String("completed".to_owned()),
            ),
            (
                sim_kernel::Symbol::new("selector"),
                Datum::String(
                    "git/branch/main git/direct/v1 git/peeled/v1 git/tag/v1 registry/checksum/sim-kernel"
                        .to_owned(),
                ),
            ),
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
