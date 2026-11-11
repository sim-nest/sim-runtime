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

const COMPOSITION: &str = "pages 1 revision 10e8d81a27f696c6d1896b44d6327c2c7bf92b54c5ac2f938455c68787cd0b92 transitions 1 stale-effects 0 effects 1 hot-delta 0 samples 20 providers model/direction,model/landing,model/snapshot stale-edges 1 bound 1 proof 0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e linux 0";
const LINUX: &str = "linux-provider";
const FREEZE: &str = "freeze-mismatch empty freeze-mismatch";
const ADVICE: &str = "advice-rejected";
const BEGIN: &str = "distinct begin-required landed 1 stale 1 proof 0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "qualify-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "qualify gate has no owner command".into(),
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
        "qualify-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_qualify_command_matches_the_composition_laws() {
    let command = WorldCommandLib::new().expect("command");
    let composition = command.run(&argv("composition")).expect("composition");
    let linux = command.run(&argv("linux")).expect("linux");
    let freeze = command.run(&argv("freeze")).expect("freeze");
    let advice = command.run(&argv("advice")).expect("advice");
    let begin = command.run(&argv("begin")).expect("begin");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command.run(&argv("missing")).expect("missing qualify case");
    assert_eq!(text(&composition, "detail"), COMPOSITION);
    assert_eq!(count(&composition, "invokes"), "1");
    assert_eq!(text(&linux, "detail"), LINUX);
    assert_eq!(count(&linux, "invokes"), "0");
    assert_eq!(text(&freeze, "detail"), FREEZE);
    assert_eq!(count(&freeze, "invokes"), "0");
    assert_eq!(text(&advice, "detail"), ADVICE);
    assert_eq!(count(&advice, "invokes"), "0");
    assert_eq!(text(&begin, "detail"), BEGIN);
    assert_eq!(count(&begin, "invokes"), "1");
    assert_eq!(count(&composition, "effects"), "0");
    assert_eq!(count(&linux, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let again = command.run(&argv("linux")).expect("linux again");
    assert_eq!(linux, again);
    let leaf = qualify_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("composition gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/26");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("qualify composition {COMPOSITION}")));
    assert!(present_claims.contains(&format!("qualify linux {LINUX}")));
    assert!(present_claims.contains(&format!("qualify freeze {FREEZE}")));
    assert!(present_claims.contains(&format!("qualify advice {ADVICE}")));
    assert!(present_claims.contains(&format!("qualify begin {BEGIN}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_qualify_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[1].value = with_field(
        &captured.rows[1].value,
        "detail",
        Datum::String("linux-admitted".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("composition gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("linux-admitted"));
    assert!(!present_claims.contains(&format!("qualify linux {LINUX}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_qualify_command_calls_the_published_specimen() {
    let source = include_str!("qualify_drive.rs");
    assert!(source.contains("qualify("));
    assert!(source.contains("compiled_executor_closure("));
    assert!(source.contains("open_with_linux("));
    assert!(source.contains("advice_rejected("));
    assert!(source.contains("selection_is_distinct("));
    assert!(source.contains("hot_needs_begin("));
    assert!(source.contains("selection_without_begin("));
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
