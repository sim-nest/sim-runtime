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

const STOP: &str = "ambiguous unreconciled unresolved changed same 1";
const ADMIT: &str = "not-retryable unnamed exhausted retry same 1";
const DESCENT: &str = "refiner terminal same 1";
const IMAGE: &str = "unchanged preimage postimage foreign same 1";
const PLAN: &str = "committed apply ambiguous same 1";
const PRIVATE: &str = "private-material:s3cret-token";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "recover-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "recovery gate has no owner command".into(),
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
        "recover-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "recover".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_recover_command_matches_the_recovery_laws() {
    let command = WorldCommandLib::new().expect("command");
    let stop = command.run(&argv("stop")).expect("stop");
    let admit = command.run(&argv("admit")).expect("admit");
    let descent = command.run(&argv("descent")).expect("descent");
    let image = command.run(&argv("image")).expect("image");
    let plan = command.run(&argv("plan")).expect("plan");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("missing")).expect("missing recover case");
    assert_eq!(text(&stop, "detail"), STOP);
    assert_eq!(count(&stop, "invokes"), "8");
    assert_eq!(text(&admit, "detail"), ADMIT);
    assert_eq!(count(&admit, "invokes"), "8");
    assert_eq!(text(&descent, "detail"), DESCENT);
    assert_eq!(count(&descent, "invokes"), "8");
    assert_eq!(text(&image, "detail"), IMAGE);
    assert_eq!(count(&image, "invokes"), "4");
    assert_eq!(text(&plan, "detail"), PLAN);
    assert_eq!(count(&plan, "invokes"), "5");
    assert_eq!(count(&stop, "effects"), "0");
    assert_eq!(count(&plan, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("stop")).expect("stop again");
    assert_eq!(stop, repeated);
    let leaf = recover_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("recovery gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/05");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("recover stop {STOP}")));
    assert!(present_claims.contains(&format!("recover admit {ADMIT}")));
    assert!(present_claims.contains(&format!("recover descent {DESCENT}")));
    assert!(present_claims.contains(&format!("recover image {IMAGE}")));
    assert!(present_claims.contains(&format!("recover plan {PLAN}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_recover_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[2].value = with_field(
        &captured.rows[2].value,
        "detail",
        Datum::String("descent-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("recovery gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("descent-kept"));
    assert!(!present_claims.contains(&format!("recover descent {DESCENT}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_recover_command_calls_the_published_recovery() {
    let source = include_str!("recover_drive.rs");
    assert!(source.contains("plan_retry_after_reconciliation("));
    assert!(source.contains("plan_refinement_after_reconciliation("));
    assert!(source.contains("classify_image("));
    assert!(source.contains("classify_plan("));
    assert!(source.contains("inverse_plan("));
    assert!(source.contains("terminal_requests_no_effects("));
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
