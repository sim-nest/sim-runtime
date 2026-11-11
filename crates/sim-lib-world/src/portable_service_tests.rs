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
        "service-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "service gate has no owner command".into(),
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
        "service-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_service_command_matches_the_authored_readings() {
    let command = WorldCommandLib::new().expect("command");
    let recorded = command
        .run(&snapshot_argv("model", "ci/sim-sdk"))
        .expect("snapshot");
    let planned = command.run(&plan_argv("model")).expect("plan");
    let leaf = service_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("service gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/19");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("state ci/sim-sdk uncertain"));
    assert!(present_claims.contains("omitted git/branch/main"));
    assert!(present_claims.contains("action git/branch/topic divergent"));
    assert!(present_claims.contains("kept git/branch/topic divergent"));
    assert!(present_claims.contains("native git/branch/topic unavailable"));
    assert!(present_claims.contains("real git/branch/topic unavailable"));
    assert!(
        !present_claims
            .iter()
            .any(|claim| claim.contains("omitted git/branch/topic"))
    );
    assert_eq!(field_count(&recorded, "effects"), "0");
    assert_eq!(field_count(&recorded, "subprocesses"), "0");
    assert_eq!(field_count(&planned, "effects"), "0");
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_state_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    let index = row_index(&captured.rows, "ci/sim-sdk");
    captured.rows[index] = with_observation_state(&captured.rows[index], "current");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        expected_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("service gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("row model ci/sim-sdk state current"));
    assert_ne!(actual_deck, expected_deck);
    assert!(!present_claims.contains("state ci/sim-sdk uncertain"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn misattributed_reads_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    let main = row_index(&captured.rows, "git/branch/main");
    let topic = row_index(&captured.rows, "git/branch/topic");
    captured.rows[main] = with_field(&captured.rows[main], "reads", Datum::Vector(Vec::new()));
    captured.rows[topic] = with_field(
        &captured.rows[topic],
        "reads",
        Datum::Vector(vec![
            Datum::String("git/branch/main".to_owned()),
            Datum::String("git/branch/topic".to_owned()),
        ]),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("service gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("row model git/branch/main state current port 1|git provenance 1|model/git/branch/main value 1|model-commit-main reads 0"));
    assert!(actual_deck.contains("reads 2|git/branch/main|git/branch/topic"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn an_already_true_action_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    let action = Datum::Node {
        tag: sim_kernel::Symbol::qualified("world", "service-action-v1"),
        fields: vec![
            (
                sim_kernel::Symbol::new("class"),
                Datum::String("divergent".to_owned()),
            ),
            (
                sim_kernel::Symbol::new("selector"),
                Datum::String("git/branch/main".to_owned()),
            ),
        ],
    };
    captured.model_plan = with_field(&captured.model_plan, "actions", Datum::Vector(vec![action]));
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("service gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("actions 1|git/branch/main:divergent"));
    assert!(!present_claims.contains("action git/branch/topic divergent"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn blank_and_joined_tokens_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut native = capture(&command).expect("capture");
    native.native = with_field(
        &native.native,
        "reads",
        Datum::Vector(vec![Datum::String(String::new())]),
    );
    let native_leaf = leaf_from(native);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &native_leaf else {
        panic!("service gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("row native git/branch/topic state unavailable port 1|native provenance 1|native-capability-missing value 0 reads 1|"));
    assert_eq!(disposition(native_leaf), ProofDisposition::Failed);

    let mut joined = capture(&command).expect("capture");
    joined.model_plan = with_field(
        &joined.model_plan,
        "omitted",
        Datum::Vector(vec![Datum::String(
            "git/branch/main git/direct/v1 git/peeled/v1 git/tag/v1 registry/checksum/sim-kernel"
                .to_owned(),
        )]),
    );
    let joined_leaf = leaf_from(joined);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &joined_leaf else {
        panic!("service gate is a source-deck predicate");
    };
    assert!(actual_deck.contains(
        "omitted 1|git/branch/main git/direct/v1 git/peeled/v1 git/tag/v1 registry/checksum/sim-kernel"
    ));
    assert_eq!(disposition(joined_leaf), ProofDisposition::Failed);
}

fn row_index(rows: &[Datum], selector: &str) -> usize {
    rows.iter()
        .position(|row| field_text(row, "selector") == selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn with_observation_state(value: &Datum, state: &str) -> Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("node");
    };
    let mut found = false;
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == "observations" {
                found = true;
                (field.clone(), replace_state(item, state))
            } else {
                (field.clone(), item.clone())
            }
        })
        .collect();
    assert!(found, "missing observations");
    Datum::Node {
        tag: tag.clone(),
        fields,
    }
}

fn replace_state(value: &Datum, state: &str) -> Datum {
    let Datum::Vector(items) = value else {
        panic!("observations");
    };
    let mut items = items.clone();
    let first = items.first().expect("observation").clone();
    items[0] = with_field(&first, "state", Datum::String(state.to_owned()));
    Datum::Vector(items)
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
