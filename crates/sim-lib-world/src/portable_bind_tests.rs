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

const SERVE: &str = "pure effects distinct same 1";
const SWAPPED: &str = "wrong-binding same 1";
const MISSING: &str = "unimplemented wrong-scope same 1";
const FACET: &str = "pure landing same 1";
const PRIVATE: &str = "private-material:s3cret-token";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "bind-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "binding gate has no owner command".into(),
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
        "bind-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "bind".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_bind_command_matches_the_binding_laws() {
    let command = WorldCommandLib::new().expect("command");
    let serve = command.run(&argv("serve")).expect("serve");
    let swapped = command.run(&argv("swapped")).expect("swapped");
    let missing = command.run(&argv("missing")).expect("missing");
    let facet = command.run(&argv("facet")).expect("facet");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("absent")).expect("absent bind case");
    assert_eq!(text(&serve, "detail"), SERVE);
    assert_eq!(count(&serve, "invokes"), "4");
    assert_eq!(text(&swapped, "detail"), SWAPPED);
    assert_eq!(count(&swapped, "invokes"), "1");
    assert_eq!(text(&missing, "detail"), MISSING);
    assert_eq!(count(&missing, "invokes"), "2");
    assert_eq!(text(&facet, "detail"), FACET);
    assert_eq!(count(&facet, "invokes"), "2");
    assert_eq!(count(&serve, "effects"), "0");
    assert_eq!(count(&facet, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    assert_eq!(text(&unknown, "case"), "absent");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("serve")).expect("serve again");
    assert_eq!(serve, repeated);
    let leaf = bind_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("binding gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/01");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("bind serve {SERVE}")));
    assert!(present_claims.contains(&format!("bind swapped {SWAPPED}")));
    assert!(present_claims.contains(&format!("bind missing {MISSING}")));
    assert!(present_claims.contains(&format!("bind facet {FACET}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_bind_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[1].value = with_field(
        &captured.rows[1].value,
        "detail",
        Datum::String("swapped-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("binding gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("swapped-kept"));
    assert!(!present_claims.contains(&format!("bind swapped {SWAPPED}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_bind_command_calls_the_published_binding() {
    let source = include_str!("bind_drive.rs");
    assert!(source.contains("packs::work::check("));
    assert!(source.contains("packs::facet::check("));
    assert!(source.contains("checker_binding("));
    assert!(source.contains("find_pack("));
    assert!(!source.contains("ModelWorker"));
    assert!(!source.contains("MutationEngine"));
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
