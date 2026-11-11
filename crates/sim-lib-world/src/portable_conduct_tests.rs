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

const QUIET: &str = "quiescent invokes 0 journal same state planned";
const ONE: &str = "advanced invokes 1 inspected still state running";
const OFFER: &str =
    "blocked quiescent zero 0 wake resource-offer advanced quiescent invokes 1 leases empty";
const DIRECTION: &str = "need-direction need-direction journal same wrong-direction advisory-direction stale-direction journal same direction-authority advanced state planned quiescent journal same replayed-direction advanced invokes 1 state running";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "conduct-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "conduct gate has no owner command".into(),
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
        "conduct-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_conduct_command_matches_the_one_effect_laws() {
    let command = WorldCommandLib::new().expect("command");
    let quiet = command.run(&argv("quiet")).expect("quiet");
    let one = command.run(&argv("one")).expect("one");
    let offer = command.run(&argv("offer")).expect("offer");
    let direction = command.run(&argv("direction")).expect("direction");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("missing")).expect("missing conduct case");
    assert_eq!(text(&quiet, "detail"), QUIET);
    assert_eq!(count(&quiet, "invokes"), "0");
    assert_eq!(text(&one, "detail"), ONE);
    assert_eq!(count(&one, "invokes"), "1");
    assert_eq!(text(&offer, "detail"), OFFER);
    assert_eq!(count(&offer, "invokes"), "1");
    assert_eq!(text(&direction, "detail"), DIRECTION);
    assert_eq!(count(&direction, "invokes"), "1");
    assert_eq!(count(&quiet, "effects"), "0");
    assert_eq!(count(&one, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let again = command.run(&argv("one")).expect("one again");
    assert_eq!(one, again);
    let leaf = conduct_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("conduct gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/25");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("conduct quiet {QUIET}")));
    assert!(present_claims.contains(&format!("conduct one {ONE}")));
    assert!(present_claims.contains(&format!("conduct offer {OFFER}")));
    assert!(present_claims.contains(&format!("conduct direction {DIRECTION}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_conduct_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[1].value = with_field(
        &captured.rows[1].value,
        "detail",
        Datum::String("advanced invokes 2 inspected moved state running".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("conduct gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("invokes 2"));
    assert!(!present_claims.contains(&format!("conduct one {ONE}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_conduct_command_calls_the_published_conductor() {
    let source = include_str!("conduct_drive.rs");
    assert!(source.contains("conduct_one("));
    assert!(!source.contains("advance_one_effect"));
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
