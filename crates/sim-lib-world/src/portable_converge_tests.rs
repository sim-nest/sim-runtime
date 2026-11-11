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

const PRESERVE: &str = "self-approval held 1 uncertain 1 same 1 later uncertain";
const INVALIDATE: &str = "affected a closed b changed 1";
const OSCILLATE: &str = "empty 1 seen note same 1";
const PARALLEL: &str = "same 1 failures a:proof-failed,b:proof-failed";
const HONEST: &str = "incomplete need malformed bad same 1";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "converge-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "converge gate has no owner command".into(),
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
        "converge-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_converge_command_matches_the_revision_laws() {
    let command = WorldCommandLib::new().expect("command");
    let preserve = command.run(&argv("preserve")).expect("preserve");
    let invalidate = command.run(&argv("invalidate")).expect("invalidate");
    let oscillate = command.run(&argv("oscillate")).expect("oscillate");
    let parallel = command.run(&argv("parallel")).expect("parallel");
    let honest = command.run(&argv("honest")).expect("honest");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command
        .run(&argv("missing"))
        .expect("missing converge case");
    assert_eq!(text(&preserve, "detail"), PRESERVE);
    assert_eq!(count(&preserve, "invokes"), "5");
    assert_eq!(text(&invalidate, "detail"), INVALIDATE);
    assert_eq!(count(&invalidate, "invokes"), "3");
    assert_eq!(text(&oscillate, "detail"), OSCILLATE);
    assert_eq!(count(&oscillate, "invokes"), "5");
    assert_eq!(text(&parallel, "detail"), PARALLEL);
    assert_eq!(count(&parallel, "invokes"), "2");
    assert_eq!(text(&honest, "detail"), HONEST);
    assert_eq!(count(&honest, "invokes"), "4");
    assert_eq!(count(&preserve, "effects"), "0");
    assert_eq!(count(&parallel, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let again = command.run(&argv("parallel")).expect("parallel again");
    assert_eq!(parallel, again);
    let leaf = converge_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("convergence gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/15");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("converge preserve {PRESERVE}")));
    assert!(present_claims.contains(&format!("converge invalidate {INVALIDATE}")));
    assert!(present_claims.contains(&format!("converge oscillate {OSCILLATE}")));
    assert!(present_claims.contains(&format!("converge parallel {PARALLEL}")));
    assert!(present_claims.contains(&format!("converge honest {HONEST}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_converge_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[3].value = with_field(
        &captured.rows[3].value,
        "detail",
        Datum::String("parallel-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("convergence gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("parallel-kept"));
    assert!(!present_claims.contains(&format!("converge parallel {PARALLEL}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_converge_command_calls_the_published_revision() {
    let source = include_str!("converge_drive.rs");
    assert!(source.contains("open_revision("));
    assert!(source.contains("propose("));
    assert!(source.contains("proof_frontier("));
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
