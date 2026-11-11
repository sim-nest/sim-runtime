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

const FRONTIER: &str = "same 1 partial a:proof-failed order a,c,b zero b,a cycles a:dependency-cycle,b:dependency-cycle,m:missing-dependency";
const RESUME: &str = "failures 3 open 0 same 1";
const WAIT: &str = "admitted b held-a 0 cpu 1 disk 0 waiting a gpu";
const CONTROL: &str = "queued 8 stopped 0 log stop os 0";
const LIVENESS: &str = "stalled s0 since 0 failures 0 work 1 idle 1 beats 7 closed 1 os 0 wake resource-offer,control,fact-refresh";

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "schedule-gate-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "schedule gate has no owner command".into(),
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
        "schedule-gate",
    )
    .expect("leaf");
    receipt.disposition
}

#[test]
fn the_loaded_schedule_command_matches_the_scheduler_laws() {
    let command = WorldCommandLib::new().expect("command");
    let frontier = command.run(&argv("frontier")).expect("frontier");
    let resume = command.run(&argv("resume")).expect("resume");
    let wait = command.run(&argv("wait")).expect("wait");
    let control = command.run(&argv("control")).expect("control");
    let liveness = command.run(&argv("liveness")).expect("liveness");
    let private_row = command.run(&argv("private")).expect("private");
    let unknown = command
        .run(&argv("missing"))
        .expect("missing schedule case");
    assert_eq!(text(&frontier, "detail"), FRONTIER);
    assert_eq!(count(&frontier, "invokes"), "8");
    assert_eq!(text(&resume, "detail"), RESUME);
    assert_eq!(count(&resume, "invokes"), "4");
    assert_eq!(text(&wait, "detail"), WAIT);
    assert_eq!(count(&wait, "invokes"), "2");
    assert_eq!(text(&control, "detail"), CONTROL);
    assert_eq!(count(&control, "invokes"), "9");
    assert_eq!(text(&liveness, "detail"), LIVENESS);
    assert_eq!(count(&liveness, "invokes"), "10");
    assert_eq!(count(&frontier, "effects"), "0");
    assert_eq!(count(&wait, "subprocesses"), "0");
    assert_eq!(text(&private_row, "reason"), "private-leakage");
    assert_eq!(text(&private_row, "case"), "");
    assert_eq!(text(&unknown, "reason"), "unknown-case");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    let again = command.run(&argv("wait")).expect("wait again");
    assert_eq!(wait, again);
    let leaf = schedule_leaf().expect("leaf");
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        present_claims,
        required_claims,
        ..
    } = &leaf
    else {
        panic!("scheduler gate is a source-deck predicate");
    };
    assert_eq!(name, "phase/13");
    assert_eq!(actual_deck, expected_deck);
    assert_eq!(present_claims, required_claims);
    assert!(present_claims.contains(&format!("schedule frontier {FRONTIER}")));
    assert!(present_claims.contains(&format!("schedule resume {RESUME}")));
    assert!(present_claims.contains(&format!("schedule wait {WAIT}")));
    assert!(present_claims.contains(&format!("schedule control {CONTROL}")));
    assert!(present_claims.contains(&format!("schedule liveness {LIVENESS}")));
    assert!(present_claims.contains("gap private private-leakage"));
    assert!(!actual_deck.contains(PRIVATE));
    assert!(!actual_deck.contains("s3cret-token"));
    assert!(!actual_deck.contains("leaked"));
    assert!(!actual_deck.contains("unstable"));
    assert!(actual_deck.starts_with("entry cli/main/world\neffects 0\n"));
    assert_eq!(disposition(leaf), ProofDisposition::Passed);
}

#[test]
fn a_changed_schedule_detail_fails() {
    let command = WorldCommandLib::new().expect("command");
    let mut captured = capture(&command).expect("capture");
    captured.rows[2].value = with_field(
        &captured.rows[2].value,
        "detail",
        Datum::String("wait-kept".to_owned()),
    );
    let leaf = leaf_from(captured);
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        present_claims,
        ..
    } = &leaf
    else {
        panic!("scheduler gate is a source-deck predicate");
    };
    assert!(actual_deck.contains("wait-kept"));
    assert!(!present_claims.contains(&format!("schedule wait {WAIT}")));
    assert_eq!(disposition(leaf), ProofDisposition::Failed);
}

#[test]
fn the_schedule_command_calls_the_published_scheduler() {
    let source = include_str!("schedule_drive.rs");
    assert!(source.contains("failure_frontier("));
    assert!(source.contains("resume_frontier("));
    assert!(source.contains("legal_order("));
    assert!(source.contains("admit_next("));
    assert!(source.contains("submit("));
    assert!(source.contains("observe_time("));
    assert!(source.contains("close_modeled_resources("));
    assert!(source.contains("os_terminated("));
    assert!(!source.contains("advance_one_effect"));
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
