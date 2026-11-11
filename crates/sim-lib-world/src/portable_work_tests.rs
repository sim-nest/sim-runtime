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
        "work-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "work gate has no owner command".into(),
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
        "work-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_work_command_matches_the_authored_work() {
    let command = WorldCommandLib::new().expect("command");
    let admitted = command.run(&argv("admitted")).expect("admitted");
    let oversize = command.run(&argv("oversize")).expect("oversize");
    let incomplete = command.run(&argv("incomplete")).expect("incomplete");
    let malformed = command.run(&argv("malformed")).expect("malformed");
    let descent = command.run(&argv("descent")).expect("descent");
    let namespace = command.run(&argv("namespace")).expect("namespace");
    let role = command.run(&argv("role")).expect("role");
    let stale = command.run(&argv("stale")).expect("stale");
    let schema = command.run(&argv("schema")).expect("schema");
    let effects = command.run(&argv("effects")).expect("effects");
    let private_row = command.run(&argv("private")).expect("private");
    assert_eq!(text(&admitted, "class"), "admitted");
    assert_eq!(text(&admitted, "proposal"), "mapped");
    assert_eq!(text(&admitted, "path"), "crates/widget/src/lib.rs");
    assert_eq!(text(&admitted, "role"), "funded-target");
    assert_eq!(text(&admitted, "status"), "declared");
    assert_eq!(text(&admitted, "receiver"), "work/api/projection");
    assert_eq!(text(&admitted, "packet"), "present");
    assert_eq!(count(&admitted, "bytes"), "12");
    assert_eq!(count(&admitted, "files"), "1");
    assert_eq!(count(&admitted, "tokens"), "12");
    assert_eq!(count(&admitted, "reads"), "1");
    assert_eq!(count(&admitted, "attempts"), "1");
    assert_eq!(count(&admitted, "worker-calls"), "1");
    assert_eq!(count(&admitted, "proposal-calls"), "1");
    assert_eq!(count(&admitted, "effects"), "0");
    assert_eq!(count(&admitted, "subprocesses"), "0");
    assert_eq!(text(&oversize, "reason"), "oversize");
    assert_eq!(count(&oversize, "reads"), "0");
    assert_eq!(count(&oversize, "worker-calls"), "0");
    assert_eq!(count(&oversize, "proposal-calls"), "0");
    assert_eq!(text(&incomplete, "class"), "incomplete");
    assert_eq!(text(&incomplete, "partial"), "partial-map");
    assert_eq!(text(&incomplete, "packet"), "present");
    assert_eq!(count(&incomplete, "reads"), "1");
    assert_eq!(text(&malformed, "reason"), "malformed-bound");
    assert_eq!(text(&malformed, "evidence"), "second");
    assert_eq!(count(&malformed, "attempts"), "2");
    assert_eq!(count(&malformed, "reads"), "1");
    assert_eq!(count(&malformed, "worker-calls"), "2");
    assert_eq!(count(&malformed, "proposal-calls"), "2");
    assert_eq!(text(&descent, "reason"), "no-descent");
    assert_eq!(text(&descent, "evidence"), "same");
    assert_eq!(count(&descent, "reads"), "1");
    assert_eq!(count(&descent, "attempts"), "0");
    assert_eq!(text(&namespace, "reason"), "namespace");
    assert_eq!(text(&namespace, "mirrors"), "true");
    assert_eq!(count(&namespace, "reads"), "0");
    assert_eq!(text(&role, "reason"), "role");
    assert_eq!(text(&role, "mirrors"), "true");
    assert_eq!(count(&role, "reads"), "0");
    assert_eq!(text(&stale, "reason"), "stale-copy");
    assert_eq!(text(&stale, "mirrors"), "true");
    assert_eq!(count(&stale, "reads"), "0");
    assert_eq!(text(&schema, "reason"), "schema");
    assert_eq!(text(&schema, "mirrors"), "true");
    assert_eq!(count(&schema, "reads"), "0");
    assert_eq!(text(&effects, "reason"), "effects-or-approval");
    assert_eq!(count(&effects, "reads"), "1");
    assert_eq!(count(&effects, "worker-calls"), "1");
    assert_eq!(count(&effects, "proposal-calls"), "1");
    assert_eq!(count(&effects, "attempts"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(count(&private_row, "reads"), "0");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let leaf = work_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("work gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/23");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("work admitted admitted reads 1"));
    assert!(present_claims.contains("work incomplete incomplete partial partial-map"));
    assert!(present_claims.contains("gap oversize oversize"));
    assert!(present_claims.contains("gap malformed malformed-bound"));
    assert!(present_claims.contains("gap descent no-descent"));
    assert!(present_claims.contains("gap namespace namespace"));
    assert!(present_claims.contains("gap role role"));
    assert!(present_claims.contains("gap stale stale-copy"));
    assert!(present_claims.contains("gap schema schema"));
    assert!(present_claims.contains("gap effects effects-or-approval"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_class_fails() {
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
        panic!("work gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("diverged"));
    assert!(!present_claims.contains("work admitted admitted reads 1"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn an_echoed_secret_fails_without_copying_it() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[0].value = with_field(
        &captured.rows[0].value,
        "proposal",
        Datum::String(PRIVATE.to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("work gate is a source-deck predicate");
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
