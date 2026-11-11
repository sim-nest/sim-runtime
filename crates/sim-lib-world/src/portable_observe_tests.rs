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
        "observe-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "observation gate has no owner command".into(),
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
        "observe-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_snapshot_command_matches_the_authored_rows() {
    let command = WorldCommandLib::new().expect("command");
    let recorded = command
        .run(&snapshot_argv(ROWS, "filesystem/secret"))
        .expect("snapshot");
    let leaf = observe_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("observation gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/09");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("status filesystem/secret denied"));
    assert!(present_claims.contains("native filesystem/readme unavailable"));
    assert_eq!(field_count(&recorded, "effects"), "0");
    assert_eq!(field_count(&recorded, "subprocesses"), "0");
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_status_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    let index = row_index(&captured.rows, "filesystem/secret");
    captured.rows[index] = with_observation_status(&captured.rows[index], "observed");
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        expected_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("observation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("status filesystem/secret observed matched"));
    assert_ne!(actual_deck, expected_deck);
    assert!(!present_claims.contains("status filesystem/secret denied"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn a_reported_read_on_a_quiet_or_native_result_fails() {
    let command = WorldCommandLib::new().expect("command");
    let captured = capture(&command).expect("capture");
    let read = Datum::Vector(vec![Datum::String("filesystem/readme".to_owned())]);
    let mut quiet = captured;
    let native_read = read.clone();
    quiet.quiet = with_field(&quiet.quiet, "reads", read);
    let quiet_leaf = leaf_from(quiet);
    assert_ne(decks(&quiet_leaf));
    assert_eq!(disposition(quiet_leaf), ProofDisposition::Failed);

    let mut native = capture(&command).expect("capture");
    native.native = with_field(&native.native, "reads", native_read);
    let native_leaf = leaf_from(native);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &native_leaf else {
        panic!("observation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("native-reads 1|filesystem/readme"));
    assert_eq!(disposition(native_leaf), ProofDisposition::Failed);
}

#[test]
fn misattributed_reads_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    let quiet = row_index(&captured.rows, "ci/run");
    let holder = row_index(&captured.rows, "filesystem/readme");
    captured.rows[quiet] = with_field(&captured.rows[quiet], "reads", Datum::Vector(Vec::new()));
    captured.rows[holder] = with_field(
        &captured.rows[holder],
        "reads",
        Datum::Vector(vec![
            Datum::String("ci/run".to_owned()),
            Datum::String("filesystem/readme".to_owned()),
        ]),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("observation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("status ci/run uncertain matched 0"));
    assert!(
        actual_deck
            .contains("status filesystem/readme observed matched 2|ci/run|filesystem/readme")
    );
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn blank_and_joined_read_tokens_fail() {
    let command = WorldCommandLib::new().expect("command");
    let mut native = capture(&command).expect("capture");
    native.native = with_field(
        &native.native,
        "reads",
        Datum::Vector(vec![Datum::String(String::new())]),
    );
    let native_leaf = leaf_from(native);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &native_leaf else {
        panic!("observation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("native-reads 1|"));
    assert_eq!(disposition(native_leaf), ProofDisposition::Failed);

    let mut changed = capture(&command).expect("capture");
    changed.changed = with_field(
        &changed.changed,
        "reads",
        Datum::Vector(vec![Datum::String("git/HEAD git/HEAD".to_owned())]),
    );
    let changed_leaf = leaf_from(changed);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &changed_leaf else {
        panic!("observation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("diff-reads 1|git/HEAD git/HEAD"));
    assert_eq!(disposition(changed_leaf), ProofDisposition::Failed);
}

fn decks(leaf: &ProofLeaf) -> bool {
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        expected_deck,
        ..
    } = leaf
    else {
        panic!("observation gate is a source-deck predicate");
    };
    actual_deck == expected_deck
}

fn assert_ne(same: bool) {
    assert!(!same);
}

fn row_index(rows: &[Datum], selector: &str) -> usize {
    rows.iter()
        .position(|row| field_text(row, "selector") == selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn with_observation_status(value: &Datum, status: &str) -> Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("node");
    };
    let mut found = false;
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == "observations" {
                found = true;
                (field.clone(), replace_status(item, status))
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

fn replace_status(value: &Datum, status: &str) -> Datum {
    let Datum::Vector(items) = value else {
        panic!("observations");
    };
    let mut items = items.clone();
    let first = items.first().expect("observation").clone();
    items[0] = with_field(&first, "status", Datum::String(status.to_owned()));
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
