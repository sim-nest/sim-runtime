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

const APPLY: &str = "applied 1 unchanged same 1";
const REFUSE: &str =
    "wrong-head dirt private-flow generated-owner-bypass stale-base region-escape same 1";
const OVERLAP: &str = "overlap same 1";
const RECOVER: &str = "recovered overlap same 1";
const ROLLBACK: &str = "restored same 1 kept 1";
const PRIVATE: &str = "private-material:s3cret-token";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "transact-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "transact gate has no owner command".into(),
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
        "transact-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "transact".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_transact_command_matches_the_transaction_laws() {
    let command = WorldCommandLib::new().expect("command");
    let apply = command.run(&argv("apply")).expect("apply");
    let refuse = command.run(&argv("refuse")).expect("refuse");
    let overlap = command.run(&argv("overlap")).expect("overlap");
    let recover = command.run(&argv("recover")).expect("recover");
    let rollback = command.run(&argv("rollback")).expect("rollback");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command
        .run(&argv("missing"))
        .expect("missing transact case");
    assert_eq!(text(&apply, "detail"), APPLY);
    assert_eq!(count(&apply, "invokes"), "2");
    assert_eq!(text(&refuse, "detail"), REFUSE);
    assert_eq!(count(&refuse, "invokes"), "6");
    assert_eq!(text(&overlap, "detail"), OVERLAP);
    assert_eq!(count(&overlap, "invokes"), "2");
    assert_eq!(text(&recover, "detail"), RECOVER);
    assert_eq!(count(&recover, "invokes"), "2");
    assert_eq!(text(&rollback, "detail"), ROLLBACK);
    assert_eq!(count(&rollback, "invokes"), "2");
    assert_eq!(count(&apply, "effects"), "0");
    assert_eq!(count(&rollback, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("overlap")).expect("overlap again");
    assert_eq!(overlap, repeated);
    let leaf = transact_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("transaction gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/17");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("transact apply {APPLY}")));
    assert!(present_claims.contains(&format!("transact refuse {REFUSE}")));
    assert!(present_claims.contains(&format!("transact overlap {OVERLAP}")));
    assert!(present_claims.contains(&format!("transact recover {RECOVER}")));
    assert!(present_claims.contains(&format!("transact rollback {ROLLBACK}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_transact_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[3].value = with_field(
        &captured.rows[3].value,
        "detail",
        Datum::String("recover-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("transaction gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("recover-kept"));
    assert!(!present_claims.contains(&format!("transact recover {RECOVER}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_transact_command_calls_the_published_transaction() {
    let source = include_str!("transact_drive.rs");
    assert!(source.contains("transact_facets("));
    assert!(source.contains("rollback_model("));
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

fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, field)| field)
}
