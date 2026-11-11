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
use crate::command::{WorldCommandLib, render};

const AGAIN: &str = "sealed 2 zero same 1";
const DELETE: &str = "closure claim/alpha,claim/extra,fact/alpha,fact/extra retained fact/beta";
const GAP_DETAIL: &str = "proof-failed claim/alpha,claim/beta";
const OSCILLATE: &str = "oscillation fact/alpha same 1";
const RETIRE: &str = "retired 1 duplicate 0";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "fixed-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "fixed gate has no owner command".into(),
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
        "fixed-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_fixed_command_matches_the_fixed_point_laws() {
    let command = WorldCommandLib::new().expect("command");
    let again = command.run(&argv("again")).expect("again");
    let delete = command.run(&argv("delete")).expect("delete");
    let oscillate = command.run(&argv("oscillate")).expect("oscillate");
    let gap = command.run(&argv("gap")).expect("gap");
    let retire = command.run(&argv("retire")).expect("retire");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("missing")).expect("missing fixed case");
    assert_eq!(text(&again, "detail"), AGAIN);
    assert_eq!(count(&again, "invokes"), "4");
    assert_eq!(text(&delete, "detail"), DELETE);
    assert_eq!(count(&delete, "invokes"), "3");
    assert_eq!(text(&oscillate, "detail"), OSCILLATE);
    assert_eq!(count(&oscillate, "invokes"), "5");
    assert_eq!(text(&gap, "detail"), GAP_DETAIL);
    assert_eq!(count(&gap, "invokes"), "3");
    assert_eq!(text(&retire, "detail"), RETIRE);
    assert_eq!(count(&retire, "invokes"), "3");
    assert_eq!(count(&again, "effects"), "0");
    assert_eq!(count(&gap, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("gap")).expect("gap again");
    assert_eq!(gap, repeated);
    let leaf = fixed_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("fixed-point gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/16");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("fixed again {AGAIN}")));
    assert!(present_claims.contains(&format!("fixed delete {DELETE}")));
    assert!(present_claims.contains(&format!("fixed oscillate {OSCILLATE}")));
    assert!(present_claims.contains(&format!("fixed gap {GAP_DETAIL}")));
    assert!(present_claims.contains(&format!("fixed retire {RETIRE}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_fixed_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[3].value = with_field(
        &captured.rows[3].value,
        "detail",
        Datum::String("oscillation-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("fixed-point gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("oscillation-kept"));
    assert!(!present_claims.contains(&format!("fixed oscillate {OSCILLATE}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_fixed_command_calls_the_published_fixed_point() {
    let source = include_str!("fixed_drive.rs");
    assert!(source.contains("open_revision("));
    assert!(source.contains("seal_fixed_point("));
    assert!(source.contains("retire_duplicate("));
    assert!(source.contains("seal_id("));
    assert!(!source.contains("ModelWorker"));
}

fn with_field(value: &Datum, name: &str, replacement: Datum) -> Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("node");
    };
    let fields = fields
        .iter()
        .map(|(field, item)| {
            if field.to_string() == name {
                (field.clone(), replacement.clone())
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

fn text(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::String(text)) => text.clone(),
        _ => panic!("missing {name} in {value:?}"),
    }
}

fn count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Number(number)) => number.canonical.clone(),
        _ => panic!("missing {name} in {value:?}"),
    }
}
