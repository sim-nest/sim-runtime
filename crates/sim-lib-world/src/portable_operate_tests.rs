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

const PRIVATE: &str = "private-material:s3cret-token";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "operate-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "operation gate has no owner command".into(),
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
        "operate-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "operate".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_operate_command_matches_the_operation_law() {
    let command = WorldCommandLib::new().expect("command");
    let measure = command.run(&argv("measure")).expect("measure");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("absent")).expect("unknown operate case");
    assert_eq!(text(&measure, "verified"), "verified");
    assert_eq!(text(&measure, "replay"), "held");
    assert_eq!(text(&measure, "uncertain"), "uncertain");
    assert_eq!(text(&measure, "forged"), "refused");
    assert_eq!(text(&measure, "counter"), "same");
    assert_eq!(count(&measure, "interrupts"), "14");
    assert_eq!(count(&measure, "recovered"), "14");
    assert_eq!(count(&measure, "invokes"), "32");
    assert_eq!(count(&measure, "effects"), "0");
    assert_eq!(count(&measure, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    assert_eq!(text(&unknown, "case"), "absent");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("measure")).expect("measure again");
    assert_eq!(measure, repeated);
    let leaf = operate_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("operation gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/04");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("operate measure verified held uncertain refused"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_operation_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "verified",
        Datum::String("repeated".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("operation gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("repeated"));
    assert!(!present_claims.contains("operate measure verified held uncertain refused"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_operate_command_calls_the_operation_owner() {
    let source = include_str!("operate_drive.rs");
    assert!(source.contains("OperationLifecycle::from_shared"));
    assert!(source.contains("OperationService::from_shared"));
    assert!(source.contains("project_verified_lifecycle_records"));
    assert!(source.contains("PerformerResponse::AcknowledgementMissing"));
    assert!(source.contains("JournalError::InjectedCrash"));
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
        _ => panic!("missing {name}"),
    }
}

fn count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Number(number)) => number.canonical.clone(),
        _ => panic!("missing {name}"),
    }
}
