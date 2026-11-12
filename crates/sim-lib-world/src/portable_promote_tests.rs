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
use crate::promote::law::{self, REGISTRY};

const PRIVATE: &str = "private-material:s3cret-token";

const REFUSALS: &[(&str, &str)] = &[
    ("path", "path-source"),
    ("receipt", "local-receipt"),
    ("model", "model-plan"),
    ("version", "missing-version"),
    ("checksum", "missing-checksum"),
    ("source", "missing-source"),
    ("order", "dependency-order"),
    ("revision", "dependency-order"),
    ("linux", "linux-requirement"),
    ("install", "missing-install"),
    ("unreconciled", "unreconciled"),
    ("overlap", "overlap"),
    ("empty", "missing-package"),
    ("ci", "missing-ci"),
    ("mirror", "missing-mirror"),
];

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "promote-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "promote gate has no owner command".into(),
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
        "promote-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "promote".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_promote_command_matches_the_registry_law() {
    let command = WorldCommandLib::new().expect("command");
    let registry = command.run(&argv("registry")).expect("registry");
    let admitted = law::admit(&REGISTRY).expect("census");
    assert_eq!(count(&registry, "packages"), admitted.packages.to_string());
    assert_eq!(count(&registry, "needs"), admitted.needs.to_string());
    assert_eq!(
        count(&registry, "packages"),
        REGISTRY.artifacts.len().to_string()
    );
    assert_eq!(count(&registry, "needs"), REGISTRY.needs.len().to_string());
    assert_eq!(count(&registry, "effects"), "0");
    assert_eq!(count(&registry, "subprocesses"), "0");
    assert!(flag(&registry, "resident"));
    let repeated = command.run(&argv("registry")).expect("registry again");
    assert_eq!(registry, repeated);
    for (case, reason) in REFUSALS {
        let record = command.run(&argv(case)).expect(case);
        let set = law::table(case).expect("set");
        let fault = law::admit(&set).expect_err("refused");
        assert_eq!(text(&record, "reason"), fault.code());
        assert_eq!(text(&record, "reason"), *reason);
        assert_eq!(text(&record, "case"), *case);
        assert_eq!(count(&record, "effects"), "0");
        assert_eq!(count(&record, "subprocesses"), "0");
    }
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("absent")).expect("absent promote case");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    assert_eq!(text(&unknown, "case"), "absent");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let source = include_str!("promote.rs");
    let law_source = include_str!("promote_admit.rs");
    assert!(source.contains("law::admit(&set)"));
    assert!(!source.contains("std::process"));
    assert!(!law_source.contains("std::process"));
    assert!(!law_source.contains("crates.io"));
    assert!(!law_source.contains("Command::new"));
    let leaf = promote_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("promote gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/27");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!(
        "promote registry packages {} needs {}",
        REGISTRY.artifacts.len(),
        REGISTRY.needs.len()
    )));
    assert!(present_claims.contains("gap path path-source"));
    assert!(present_claims.contains("gap receipt local-receipt"));
    assert!(present_claims.contains("gap model model-plan"));
    assert!(present_claims.contains("gap linux linux-requirement"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_promote_count_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "packages",
        crate::query::count_datum(0),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("promote gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("promote registry packages 0 needs"));
    assert!(!present_claims.contains(&format!(
        "promote registry packages {} needs {}",
        REGISTRY.artifacts.len(),
        REGISTRY.needs.len()
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
