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
use crate::manifest::law::{self, OWNER};

const PRIVATE: &str = "private-material:s3cret-token";

const REFUSALS: &[(&str, &str)] = &[
    ("pin-drift", "pin-drift"),
    ("missing-pin", "missing-pin"),
    ("private-pin", "private-pin"),
    ("escape", "escape"),
    ("split-source", "split-source"),
    ("overlap", "overlap"),
    ("missing-owner", "missing-owner"),
    ("cycle", "cycle"),
    ("linux-target", "linux-target"),
    ("fabricated", "fabricated"),
];

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "manifest-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "manifest gate has no owner command".into(),
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
        "manifest-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "manifest".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_manifest_command_matches_the_owner_law() {
    let command = WorldCommandLib::new().expect("command");
    let owner = command.run(&argv("owner")).expect("owner");
    let admitted = law::admit(&OWNER).expect("census");
    assert_eq!(count(&owner, "owners"), admitted.owners.to_string());
    assert_eq!(count(&owner, "packages"), admitted.packages.to_string());
    assert_eq!(count(&owner, "edges"), admitted.edges.to_string());
    assert_eq!(count(&owner, "owners"), OWNER.owners.len().to_string());
    assert_eq!(count(&owner, "packages"), OWNER.packages.len().to_string());
    assert_eq!(count(&owner, "edges"), OWNER.edges.len().to_string());
    assert_eq!(count(&owner, "effects"), "0");
    assert_eq!(count(&owner, "subprocesses"), "0");
    assert!(flag(&owner, "resident"));
    let repeated = command.run(&argv("owner")).expect("owner again");
    assert_eq!(owner, repeated);
    for (case, reason) in REFUSALS {
        let record = command.run(&argv(case)).expect(case);
        let table = law::table(case).expect("table");
        let fault = law::admit(&table).expect_err("refused");
        assert_eq!(text(&record, "reason"), fault.code());
        assert_eq!(text(&record, "reason"), *reason);
        assert_eq!(text(&record, "case"), *case);
        assert_eq!(count(&record, "effects"), "0");
        assert_eq!(count(&record, "subprocesses"), "0");
    }
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("absent")).expect("absent manifest case");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    assert_eq!(text(&unknown, "case"), "absent");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let source = include_str!("manifest.rs");
    assert!(source.contains("law::admit(&table)"));
    assert!(!source.contains("no-owner-result"));
    let leaf = manifest_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("manifest gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/00");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!(
        "manifest owner owners {} packages {} edges {}",
        OWNER.owners.len(),
        OWNER.packages.len(),
        OWNER.edges.len()
    )));
    assert!(present_claims.contains("gap cycle cycle"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_manifest_count_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "owners",
        crate::query::count_datum(0),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("manifest gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("manifest owner owners 0 packages"));
    assert!(!present_claims.contains(&format!(
        "manifest owner owners {} packages {} edges {}",
        OWNER.owners.len(),
        OWNER.packages.len(),
        OWNER.edges.len()
    )));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
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

fn flag(value: &Datum, name: &str) -> bool {
    match field(value, name) {
        Some(Datum::Bool(flag)) => *flag,
        _ => panic!("missing {name} in {value:?}"),
    }
}
