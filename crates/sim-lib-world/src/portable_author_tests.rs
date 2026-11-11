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

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "author-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "author gate has no owner command".into(),
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
        "author-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_author_command_matches_the_authored_family() {
    let command = WorldCommandLib::new().expect("command");
    let selected = command.run(&argv("selected")).expect("selected");
    let consulted = command.run(&argv("consulted")).expect("consulted");
    let superseded = command.run(&argv("superseded")).expect("superseded");
    let argument = command.run(&argv("argument")).expect("argument");
    let work = command.run(&argv("work")).expect("work");
    let gate = command.run(&argv("gate")).expect("gate");
    let ratchet = command.run(&argv("ratchet")).expect("ratchet");
    let owner = command.run(&argv("owner")).expect("owner");
    let checker = command.run(&argv("checker")).expect("checker");
    let authority = command.run(&argv("authority")).expect("authority");
    let private_row = command.run(&argv("private")).expect("private");
    assert_eq!(text(&selected, "class"), "selected");
    assert_eq!(text(&selected, "shown"), "chosen");
    assert_eq!(text(&selected, "lint"), "ok");
    assert_eq!(text(&selected, "selection"), "chosen");
    assert_eq!(
        text(&selected, "changed"),
        "name,round,stance,path,ratchet,checker,consult"
    );
    assert_eq!(text(&selected, "round-trip"), "true");
    assert_eq!(text(&selected, "shapes"), "distinct");
    assert_eq!(text(&selected, "compiled"), "true");
    assert_eq!(text(&selected, "proof"), "source-deck");
    assert_eq!(text(&selected, "phase"), "authoring");
    assert_eq!(text(&selected, "uncertain"), "false");
    assert_eq!(text(&selected, "closed"), "true");
    assert_eq!(text(&selected, "grounded"), "true");
    assert_eq!(count(&selected, "members"), "2");
    assert_eq!(count(&selected, "consultation"), "1");
    assert_eq!(count(&selected, "bytes"), "278");
    assert_eq!(count(&selected, "claims"), "1");
    assert_eq!(count(&selected, "entries"), "1");
    assert_eq!(count(&selected, "postimage"), "278");
    assert_eq!(count(&selected, "effects"), "0");
    assert_eq!(count(&selected, "subprocesses"), "0");
    assert_eq!(text(&consulted, "reason"), "unsatisfied");
    assert_eq!(text(&consulted, "evidence"), "semantic-tests");
    assert_eq!(text(&superseded, "reason"), "unsatisfied");
    assert_eq!(text(&superseded, "evidence"), "semantic-tests");
    assert_eq!(text(&argument, "reason"), "missing");
    assert_eq!(text(&argument, "evidence"), "argument");
    assert_eq!(text(&work, "reason"), "missing");
    assert_eq!(text(&work, "evidence"), "work");
    assert_eq!(text(&gate, "reason"), "missing");
    assert_eq!(text(&gate, "evidence"), "gate");
    assert_eq!(text(&ratchet, "reason"), "missing");
    assert_eq!(text(&ratchet, "evidence"), "ratchet");
    assert_eq!(text(&owner, "reason"), "missing");
    assert_eq!(text(&owner, "evidence"), "owner");
    assert_eq!(text(&checker, "reason"), "missing");
    assert_eq!(text(&checker, "evidence"), "checker");
    assert_eq!(text(&authority, "reason"), "authority");
    assert_eq!(text(&authority, "evidence"), "");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(count(&private_row, "effects"), "0");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let leaf = author_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("author gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/24");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("author selected selected members 2"));
    assert!(present_claims.contains("gap consulted unsatisfied"));
    assert!(present_claims.contains("gap superseded unsatisfied"));
    assert!(present_claims.contains("gap argument missing"));
    assert!(present_claims.contains("gap work missing"));
    assert!(present_claims.contains("gap gate missing"));
    assert!(present_claims.contains("gap ratchet missing"));
    assert!(present_claims.contains("gap owner missing"));
    assert!(present_claims.contains("gap checker missing"));
    assert!(present_claims.contains("gap authority authority"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_author_class_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "class",
        Datum::String("diverged".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("author gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("diverged"));
    assert!(!present_claims.contains("author selected selected members 2"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn an_echoed_author_secret_fails_without_copying_it() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "shown",
        Datum::String(PRIVATE.to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("author gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("leaked"));
    assert!(actual_deck.contains("hidden"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains(PRIVATE));
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
        _ => panic!("missing {name}"),
    }
}

fn count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Number(number)) => number.canonical.clone(),
        _ => panic!("missing {name}"),
    }
}
