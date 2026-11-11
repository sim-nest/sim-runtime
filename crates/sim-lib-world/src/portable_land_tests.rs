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

const FOREIGN: &str = "would-commit applied kept same 1";
const REPLAY: &str = "already-true same 1";
const RECOVER: &str = "restored exact kept 1";
const GENERATED: &str = "generated-owner-bypass same 1";
const CACHE: &str = "unknown current stale same 1";
const STATUS: &str = "present missing same 1";
const PRIVATE: &str = "private-material:s3cret-token";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "land-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "landing gate has no owner command".into(),
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
        "land-gate",
    )
    .expect("leaf");
    receipt.disposition
}

fn argv(case: &str) -> Vec<String> {
    let case = if case == "private" { PRIVATE } else { case };
    vec!["sim".to_owned(), "land".to_owned(), case.to_owned()]
}

#[test]
fn the_loaded_land_command_matches_the_landing_laws() {
    let command = WorldCommandLib::new().expect("command");
    let foreign = command.run(&argv("foreign")).expect("foreign");
    let replay = command.run(&argv("replay")).expect("replay");
    let recover = command.run(&argv("recover")).expect("recover");
    let generated = command.run(&argv("generated")).expect("generated");
    let cache = command.run(&argv("cache")).expect("cache");
    let status = command.run(&argv("status")).expect("status");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("missing")).expect("missing land case");
    assert_eq!(text(&foreign, "detail"), FOREIGN);
    assert_eq!(count(&foreign, "invokes"), "3");
    assert_eq!(text(&replay, "detail"), REPLAY);
    assert_eq!(count(&replay, "invokes"), "2");
    assert_eq!(text(&recover, "detail"), RECOVER);
    assert_eq!(count(&recover, "invokes"), "3");
    assert_eq!(text(&generated, "detail"), GENERATED);
    assert_eq!(count(&generated, "invokes"), "2");
    assert_eq!(text(&cache, "detail"), CACHE);
    assert_eq!(count(&cache, "invokes"), "6");
    assert_eq!(text(&status, "detail"), STATUS);
    assert_eq!(count(&status, "invokes"), "2");
    assert_eq!(count(&foreign, "effects"), "0");
    assert_eq!(count(&status, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let repeated = command.run(&argv("replay")).expect("replay again");
    assert_eq!(replay, repeated);
    let leaf = land_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("landing gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/18");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("land foreign {FOREIGN}")));
    assert!(present_claims.contains(&format!("land replay {REPLAY}")));
    assert!(present_claims.contains(&format!("land recover {RECOVER}")));
    assert!(present_claims.contains(&format!("land generated {GENERATED}")));
    assert!(present_claims.contains(&format!("land cache {CACHE}")));
    assert!(present_claims.contains(&format!("land status {STATUS}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_land_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[2].value = with_field(
        &captured.rows[2].value,
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
        panic!("landing gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("recover-kept"));
    assert!(!present_claims.contains(&format!("land recover {RECOVER}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_land_command_calls_the_published_landing() {
    let source = include_str!("land_drive.rs");
    assert!(source.contains("land_plan("));
    assert!(source.contains("land_apply("));
    assert!(source.contains("land_status("));
    assert!(source.contains("land_why("));
    assert!(source.contains("land_recover("));
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
