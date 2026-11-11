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
        "canary-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "canary gate has no owner command".into(),
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
        "canary-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_canary_command_matches_the_authored_canary() {
    let command = WorldCommandLib::new().expect("command");
    let fresh = command
        .run(&argv("model", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"))
        .expect("canary");
    let native = command
        .run(&argv("native", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"))
        .expect("native");
    let real = command
        .run(&argv("real", SOURCE_SELECTOR, FRESH, PURPOSE, "issued"))
        .expect("real");
    let leaf = canary_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("canary gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/22");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains("canary git/branch/canary-1 verified dispatches 1 repeat 0"));
    assert!(present_claims.contains("gap wrong wrong-purpose"));
    assert!(present_claims.contains("gap spent consumed-purpose"));
    assert!(present_claims.contains("gap branch public-target"));
    assert!(present_claims.contains("gap tag public-target"));
    assert!(present_claims.contains("gap package public-target"));
    assert!(present_claims.contains("gap merge public-target"));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(present_claims.contains("gap native unavailable-receiver"));
    assert!(present_claims.contains("gap real unavailable-receiver"));
    assert!(!actual_deck.contains("private-material:"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.contains("dispatches 1"));
    assert!(actual_deck.contains("repeat 0"));
    assert_eq!(field_count(&fresh, "effects"), "0");
    assert_eq!(field_count(&fresh, "subprocesses"), "0");
    assert_eq!(field_count(&fresh, "dispatches"), "1");
    assert_eq!(field_count(&fresh, "repeat-dispatches"), "0");
    let native_text = render(&native);
    let real_text = render(&real);
    assert!(!native_text.contains("model/"));
    assert!(!native_text.contains("model-commit"));
    assert!(!real_text.contains("model/"));
    assert!(!real_text.contains("model-commit"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_class_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.fresh = with_field(
        &captured.fresh,
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
        panic!("canary gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("diverged"));
    assert!(!present_claims.contains("canary git/branch/canary-1 verified dispatches 1 repeat 0"));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn an_echoed_secret_fails_without_copying_it() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.fresh = with_field(
        &captured.fresh,
        "realization",
        Datum::String("private-material:s3cret-token".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate { actual_deck, .. } = &leaf else {
        panic!("canary gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("leaked"));
    assert!(actual_deck.contains("hidden"));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("private-material:"));
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

fn field_count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Some(Datum::Number(number)) => number.canonical.clone(),
        _ => panic!("missing {name}"),
    }
}

fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(symbol, _)| symbol.to_string() == name)
        .map(|(_, item)| item)
}
