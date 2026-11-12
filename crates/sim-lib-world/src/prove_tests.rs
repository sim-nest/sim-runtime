// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::{BTreeMap, BTreeSet};

use crate::prove::{self, Choice};

#[test]
fn cold_execution_reads_every_declared_edge_and_keeps_native_leaves_out() {
    let facts = prove::leaf_facts().unwrap();
    assert_eq!(facts.len(), 40);
    assert_eq!(
        facts["conclusion/01"],
        vec![
            "stable/01".to_owned(),
            "change/private-implementation".to_owned(),
        ]
    );
    assert!(!facts.contains_key("leaf/native-worker-adapter"));
    assert!(!facts.contains_key("leaf/native-assay"));
    let edges: u64 = facts.values().map(|fact| fact.len() as u64).sum();
    let cold = prove::execute_cold().unwrap();
    assert_eq!(cold.leaves.len(), 40);
    let provider = crate::assay::semantic_provider_reads(crate::assay::universe().unwrap().len());
    assert_eq!(cold.requests, edges * 7 + provider);
    assert_eq!(cold.edges, edges);
    assert_ne!(
        cold.baseline_digest("conclusion/01"),
        cold.baseline_digest("conclusion/03")
    );
    let record = prove::run("cold", None).unwrap();
    assert_eq!(tag(&record), "prove/cold-v1");
}

#[test]
fn reuse_matches_the_cold_denominator_for_every_frozen_delta() {
    let cold = prove::execute_cold().unwrap();
    let expected = [
        ("D1", 10_000u16, 0u64, 0u64, BTreeSet::from([])),
        (
            "D2",
            9_500,
            0,
            4,
            BTreeSet::from(["conclusion/01".to_owned(), "conclusion/02".to_owned()]),
        ),
        ("D3", 8_000, 0, 16, BTreeSet::from([])),
        ("D4", 7_000, 0, 24, BTreeSet::from([])),
        ("D5", 9_000, 0, 8, BTreeSet::from([])),
        ("D6", 10_000, 0, 0, BTreeSet::from([])),
    ];
    for (delta, basis, false_reuse, reuse_requests, rerun_prefix) in expected {
        let report = prove::reuse(&cold, delta, prove::graph_choice)
            .unwrap()
            .unwrap_or_else(|| panic!("missing {delta}"));
        assert_eq!(report.basis, basis, "{delta}");
        assert_eq!(report.false_reuse, false_reuse, "{delta}");
        assert_eq!(report.reuse_requests, reuse_requests, "{delta}");
        assert_eq!(report.cold_requests, cold.requests, "{delta}");
        if !rerun_prefix.is_empty() {
            assert_eq!(report.rerun, rerun_prefix, "{delta}");
        }
        if delta == "D3" {
            assert_eq!(report.rerun.len(), 8, "{delta}");
            assert!(report.rerun.contains("conclusion/03"));
            assert!(report.rerun.contains("conclusion/10"));
            assert!(!report.rerun.contains("conclusion/11"));
        }
        if delta == "D4" {
            assert_eq!(report.rerun.len(), 12);
            assert!(report.rerun.contains("conclusion/11"));
            assert!(report.rerun.contains("conclusion/22"));
        }
        if delta == "D5" {
            assert_eq!(
                report.rerun,
                BTreeSet::from([
                    "conclusion/23".to_owned(),
                    "conclusion/24".to_owned(),
                    "conclusion/25".to_owned(),
                    "conclusion/26".to_owned(),
                ])
            );
        }
        if delta == "D6" {
            assert_eq!(report.revision, 1);
            assert!(report.rerun.is_empty());
        }
    }
}

#[test]
fn keeping_an_affected_leaf_or_claiming_an_unread_input_is_false_reuse() {
    let cold = prove::execute_cold().unwrap();
    let forced = prove::reuse(&cold, "D2", |leaf, affected| Choice {
        rerun: affected && leaf != "conclusion/01",
        extra_claim: None,
    })
    .unwrap()
    .unwrap();
    assert!(forced.false_reuse >= 1);
    assert!(forced.reused.contains("conclusion/01"));
    let claimed = prove::reuse(&cold, "D1", |leaf, affected| Choice {
        rerun: affected,
        extra_claim: (leaf == "conclusion/40").then(|| "change/private-implementation".to_owned()),
    })
    .unwrap()
    .unwrap();
    assert!(claimed.false_reuse >= 1);
    assert_eq!(claimed.basis, 10_000);
}

#[test]
fn an_unknown_leaf_is_a_gap_and_a_native_leaf_stays_excluded() {
    let missing = prove::frontier_leaf("leaf/missing-portable", None).unwrap();
    assert_eq!(text_field(&missing, "reason"), "unavailable-proof");
    let native = prove::frontier_leaf("leaf/native-worker-adapter", None).unwrap();
    assert_eq!(text_field(&native, "reason"), "excluded-scope");
    let assay = prove::frontier_leaf("leaf/native-assay", None).unwrap();
    assert_eq!(text_field(&assay, "reason"), "excluded-scope");
    let unknown = prove::run("D7", None).unwrap();
    assert_eq!(text_field(&unknown, "reason"), "unknown-delta");
}

#[test]
fn plan_resume_advances_one_page_and_a_bad_token_is_a_gap() {
    let first = prove::plan(None).unwrap();
    assert_eq!(tag(&first), "prove/plan-v1");
    let token = text_field(&first, "continuation").to_owned();
    assert_eq!(token.len(), 64);
    let second = prove::resume(&token, None).unwrap();
    assert_eq!(tag(&second), "prove/plan-v1");
    assert_ne!(text_field(&first, "leaves"), text_field(&second, "leaves"));
    let stale = prove::resume("stale", None).unwrap();
    assert_eq!(text_field(&stale, "reason"), "stale-or-missing");
    let zeros = "0".repeat(64);
    let wrong = prove::resume(&zeros, None).unwrap();
    assert_eq!(text_field(&wrong, "reason"), "stale-or-missing");
}

#[test]
fn frontier_of_the_cold_run_is_complete_when_no_gap_remains() {
    let cold = prove::run("cold", None).unwrap();
    assert_eq!(number_field(&cold, "denominator"), 29);
    let ran = number_field(&cold, "executed");
    assert_eq!(ran, 29);
    let view = text_field(&cold, "view");
    let executed = prove::cold_executions();
    let frontier = prove::frontier(Some(&view)).unwrap();
    assert_eq!(tag(&frontier), "prove/frontier-v1");
    assert_eq!(number_field(&frontier, "executed"), ran);
    assert_eq!(number_field(&frontier, "denominator"), 29);
    assert!(bool_field(&frontier, "complete"));
    assert!(text_field(&frontier, "gaps").is_empty());
    assert_eq!(
        number_field(&frontier, "requests"),
        number_field(&cold, "requests") * 2
    );
    let token = text_field(&frontier, "continuation").to_owned();
    let again = prove::resume(&token, Some(&view)).unwrap();
    assert_eq!(tag(&again), "prove/frontier-v1");
    assert!(bool_field(&again, "complete"));
    let missing = prove::frontier(None).unwrap();
    assert_eq!(text_field(&missing, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn cached_status_and_why_stay_inside_the_ceiling() {
    let cold = prove::run("cold", None).unwrap();
    let view = text_field(&cold, "view");
    let executed = prove::cold_executions();
    let status = prove::status(Some(&view)).unwrap();
    assert_eq!(
        number_field(&status, "requests"),
        number_field(&cold, "requests")
    );
    assert_eq!(number_field(&status, "denominator"), 29);
    assert!(bool_field(&status, "resident"));
    let missing = prove::status(None).unwrap();
    assert_eq!(text_field(&missing, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
    for command in ["status", "why"] {
        let measured = prove::measure(command).unwrap();
        let p95 = number_field(&measured.record, "p95-ms");
        assert!(
            measured.within_ceiling,
            "{command} p95 {p95} ms exceeded the 200 ms ceiling"
        );
        assert_eq!(number_field(&measured.record, "samples"), 200);
        assert_eq!(number_field(&measured.record, "warmups"), 20);
        assert_eq!(number_field(&measured.record, "records"), 29);
        assert_eq!(number_field(&measured.record, "effects"), 0);
        assert_eq!(number_field(&measured.record, "subprocesses"), 0);
        let raw = match &measured.record {
            sim_kernel::Datum::Node { fields, .. } => fields
                .iter()
                .find(|(name, _)| name.to_string() == "raw-us")
                .map(|(_, value)| value),
            _ => None,
        };
        let sim_kernel::Datum::Vector(raw) = raw.expect("raw samples") else {
            panic!("raw samples are not a vector");
        };
        assert_eq!(raw.len(), 200);
    }
}

#[test]
fn retained_view_answers_d1_without_another_cold_execution() {
    let cold = prove::run("cold", None).unwrap();
    let view = text_field(&cold, "view");
    let executed = prove::cold_executions();
    let missing = prove::run("D1", None).unwrap();
    assert_eq!(text_field(&missing, "reason"), "refresh-needed");
    let d1 = prove::run("D1", Some(&view)).unwrap();
    assert_eq!(tag(&d1), "prove/reuse-v1");
    assert_eq!(number_field(&d1, "reuse-basis-points"), 10_000);
    assert_eq!(number_field(&d1, "false-reuse"), 0);
    assert_eq!(
        number_field(&d1, "reuse-requests"),
        number_field(&cold, "requests")
    );
    let why_keep = prove::why("D1", "phase/07", Some(&view)).unwrap();
    assert!(bool_field(&why_keep, "reused"));
    assert_eq!(text_field(&why_keep, "exercised"), "");
    assert_eq!(
        number_field(&why_keep, "requests"),
        number_field(&cold, "requests")
    );
    let why_journal = prove::why("D1", "phase/02", Some(&view)).unwrap();
    assert!(bool_field(&why_journal, "reused"));
    let why_compare = prove::why("D1", "phase/14", Some(&view)).unwrap();
    assert!(bool_field(&why_compare, "reused"));
    let mut tampered = view.clone();
    let last = tampered.pop().expect("view char");
    tampered.push(if last == 'a' { 'b' } else { 'a' });
    let stale = prove::run("D1", Some(&tampered)).unwrap();
    assert_eq!(text_field(&stale, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn a_resealed_view_cannot_invent_a_failure() {
    let cold = prove::execute_cold().unwrap();
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    assert!(prove::open_view(&view).unwrap().is_some());
    let forged = prove::reseal_replaced(&view, "requests ", "failure conclusion/01;requests ")
        .unwrap()
        .expect("reseal");
    let status = prove::status(Some(&forged)).unwrap();
    assert_eq!(tag(&status), "prove/gap-v1");
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    let frontier = prove::frontier(Some(&forged)).unwrap();
    assert_eq!(text_field(&frontier, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn a_resealed_view_cannot_drop_a_failed_capture() {
    let cold = prove::execute_disturbed(prove::Disturbance {
        empty_fact: Some("stable/40".to_owned()),
        ..prove::Disturbance::none()
    })
    .unwrap();
    assert_eq!(cold.failures, vec!["conclusion/40".to_owned()]);
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    let opened = prove::open_view(&view).unwrap().expect("assay view");
    assert_eq!(opened.failures, vec!["conclusion/40".to_owned()]);
    assert_eq!(
        text_field(&prove::frontier(Some(&view)).unwrap(), "reason"),
        "refresh-needed"
    );
    let dropped_empty = prove::reseal_replaced(&view, "empty stable/40;", "")
        .unwrap()
        .expect("drop empty");
    assert_eq!(
        text_field(&prove::frontier(Some(&dropped_empty)).unwrap(), "reason"),
        "refresh-needed"
    );
    let forged = prove::reseal_replaced(&view, "failure conclusion/40;", "")
        .unwrap()
        .expect("reseal");
    let status = prove::status(Some(&forged)).unwrap();
    assert_eq!(tag(&status), "prove/gap-v1");
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    let frontier = prove::frontier(Some(&forged)).unwrap();
    assert_eq!(text_field(&frontier, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn a_changed_program_view_keeps_its_failure_after_reseal() {
    let live = code_of("conclusion/23").to_owned();
    let edited = format!("{live}\nchanged");
    let cold = prove::execute_disturbed(prove::Disturbance {
        code_for: BTreeMap::from([
            ("conclusion/23".to_owned(), edited.clone()),
            ("conclusion/24".to_owned(), edited.clone()),
        ]),
        ..prove::Disturbance::none()
    })
    .unwrap();
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    assert!(prove::open_view(&view).unwrap().is_some());
    let dropped_one = prove::reseal_replaced(&view, "failure conclusion/23;", "")
        .unwrap()
        .expect("reseal");
    let forged = prove::reseal_replaced(&dropped_one, "failure conclusion/24;", "")
        .unwrap()
        .expect("reseal");
    let status = prove::status(Some(&forged)).unwrap();
    assert_eq!(tag(&status), "prove/gap-v1");
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    let restored = prove::reseal_replaced(&view, &hex_utf8(&edited), &hex_utf8(&live))
        .unwrap()
        .expect("reseal");
    assert!(prove::open_view(&restored).unwrap().is_none());
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn a_swapped_digest_or_a_deleted_change_is_not_admitted() {
    let cold = prove::execute_cold().unwrap();
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    assert!(prove::open_view(&view).unwrap().is_some());
    let baseline = cold.baseline_digest("conclusion/01").expect("baseline");
    let other = cold.baseline_digest("conclusion/02").expect("other");
    assert_ne!(baseline, other);
    let swapped = prove::reseal_replaced(
        &view,
        &format!("baseline conclusion/01 {baseline}"),
        &format!("baseline conclusion/01 {other}"),
    )
    .unwrap()
    .expect("swap");
    assert!(prove::open_view(&swapped).unwrap().is_none());
    let changed = view
        .split(';')
        .find(|part| part.starts_with("changed D2 "))
        .expect("D2 change");
    let deleted = prove::reseal_replaced(&view, &format!("{changed};"), "")
        .unwrap()
        .expect("delete");
    assert!(prove::open_view(&deleted).unwrap().is_none());
    let requests = view
        .split(';')
        .find(|part| part.starts_with("requests "))
        .expect("requests");
    let forged_requests = prove::reseal_replaced(&view, requests, "requests 0")
        .unwrap()
        .expect("requests");
    assert!(prove::open_view(&forged_requests).unwrap().is_none());
    let forged_revision = prove::reseal_replaced(&view, "delta D1 0", "delta D1 1")
        .unwrap()
        .expect("revision");
    assert!(prove::open_view(&forged_revision).unwrap().is_none());
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn an_empty_fact_value_is_not_a_proof_view() {
    let cold = prove::execute_cold().unwrap();
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    let blank =
        prove::reseal_replaced(&view, "fact conclusion/01 stable/01", "fact conclusion/01 ")
            .unwrap()
            .expect("reseal");
    assert!(prove::open_view(&blank).unwrap().is_none());
    let status = prove::status(Some(&blank)).unwrap();
    assert_eq!(tag(&status), "prove/gap-v1");
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    let why = prove::why("D2", "phase/07", Some(&blank)).unwrap();
    assert_eq!(text_field(&why, "reason"), "refresh-needed");
    let again = prove::run("D2", Some(&blank)).unwrap();
    assert_eq!(text_field(&again, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

#[test]
fn an_empty_digest_is_not_a_proof_view() {
    let cold = prove::execute_cold().unwrap();
    let view = prove::cold_view(&cold).unwrap();
    let executed = prove::cold_executions();
    let baseline = blanked(&view, "baseline conclusion/01 ");
    let status = prove::status(Some(&baseline)).unwrap();
    assert_eq!(tag(&status), "prove/gap-v1");
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    let digest = blanked(&view, "digest D1 conclusion/01 ");
    let run = prove::run("D1", Some(&digest)).unwrap();
    assert_eq!(text_field(&run, "reason"), "refresh-needed");
    let why = prove::why("D1", "phase/07", Some(&digest)).unwrap();
    assert_eq!(text_field(&why, "reason"), "refresh-needed");
    let status = prove::status(Some(&digest)).unwrap();
    assert_eq!(text_field(&status, "reason"), "refresh-needed");
    assert_eq!(prove::cold_executions(), executed);
}

fn blanked(view: &str, prefix: &str) -> String {
    let part = view
        .split(';')
        .find(|part| part.starts_with(prefix))
        .expect("part");
    prove::reseal_replaced(view, part, prefix)
        .unwrap()
        .expect("reseal")
}

fn tag(value: &sim_kernel::Datum) -> String {
    let sim_kernel::Datum::Node { tag, .. } = value else {
        panic!("node");
    };
    tag.to_string()
}

fn text_field(value: &sim_kernel::Datum, name: &str) -> String {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    match field {
        sim_kernel::Datum::String(text) => text.clone(),
        sim_kernel::Datum::Vector(items) => items
            .iter()
            .map(|item| match item {
                sim_kernel::Datum::String(text) => text.as_str(),
                other => panic!("string item, got {other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => panic!("text field {name}: {other:?}"),
    }
}

fn bool_field(value: &sim_kernel::Datum, name: &str) -> bool {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    let sim_kernel::Datum::Bool(value) = field else {
        panic!("bool {name}");
    };
    *value
}

fn number_field(value: &sim_kernel::Datum, name: &str) -> u64 {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    let sim_kernel::Datum::Number(number) = field else {
        panic!("number {name}");
    };
    number
        .canonical
        .parse()
        .unwrap_or_else(|_| panic!("{name}"))
}

#[test]
fn the_denominator_is_the_sealed_union() {
    let facts = prove::leaf_facts().unwrap();
    let cold = prove::execute_cold().unwrap();
    assert_eq!(cold.leaves, crate::assay_oracle::obligation_leaves());
    assert_eq!(cold.leaves.len(), facts.len());
    assert!(cold.leaves.len() > 1);
    let source = include_str!("prove.rs");
    assert!(source.contains("require_union"));
    assert!(!source.contains("const DENOMINATOR"));
    assert!(!source.contains("for index in 1..=40"));
}

#[test]
fn a_hidden_read_is_false_reuse_against_that_cold_run() {
    let facts = prove::leaf_facts().unwrap();
    assert_eq!(facts["conclusion/40"], vec!["stable/40".to_owned()]);
    assert!(
        !facts["conclusion/40"]
            .iter()
            .any(|fact| fact == "change/private-implementation")
    );
    let cold = prove::execute_disturbed(prove::Disturbance {
        hidden_leaf: Some("conclusion/40".to_owned()),
        hidden_fact: Some("change/private-implementation".to_owned()),
        ..prove::Disturbance::none()
    })
    .unwrap();
    assert!(
        cold.captures["conclusion/40"].contains("change/private-implementation"),
        "{}",
        cold.captures["conclusion/40"]
    );
    let report = prove::reuse(&cold, "D2", prove::graph_choice)
        .unwrap()
        .expect("D2");
    assert_eq!(report.false_reuse, 1, "{}", report.false_reuse);
    assert!(report.reused.contains("conclusion/40"));
    assert!(!report.rerun.contains("conclusion/40"));
    let view = prove::cold_view(&cold).unwrap();
    let opened = prove::open_view(&view).unwrap().expect("hidden view");
    assert!(opened.captures["conclusion/40"].contains("change/private-implementation"));
}

#[test]
fn changed_proof_code_moves_only_the_leaves_that_consume_it() {
    let live = code_of("conclusion/23");
    let edited = format!("{live}\nchanged");
    assert_ne!(edited, live);
    let cold = prove::execute_disturbed(prove::Disturbance {
        code_for: BTreeMap::from([
            ("conclusion/23".to_owned(), edited.clone()),
            ("conclusion/24".to_owned(), edited.clone()),
        ]),
        ..prove::Disturbance::none()
    })
    .unwrap();
    let report = prove::reuse(&cold, "D6", prove::graph_choice)
        .unwrap()
        .expect("D6");
    assert_eq!(report.false_reuse, 2, "{}", report.false_reuse);
    assert_eq!(report.reuse_requests, 0);
    assert!(report.rerun.is_empty());
    assert!(report.reused.contains("conclusion/23"));
    assert!(report.reused.contains("conclusion/24"));
    assert_eq!(
        cold.failures,
        vec!["conclusion/23".to_owned(), "conclusion/24".to_owned()]
    );
    for leaf in ["conclusion/23", "conclusion/24"] {
        let own = prove::digest_for_code(leaf, live).unwrap();
        let edited_baseline = prove::digest_for_code(leaf, &edited).unwrap();
        let delta = prove::digest_for_delta(leaf, "D6", &edited).unwrap();
        assert_eq!(cold.baseline_digest(leaf), Some(own.as_str()), "{leaf}");
        assert_ne!(
            cold.baseline_digest(leaf),
            Some(edited_baseline.as_str()),
            "{leaf}"
        );
        assert_eq!(
            prove::delta_digest(&cold, "D6", leaf),
            Some(delta.as_str()),
            "{leaf}"
        );
        assert_ne!(
            prove::delta_digest(&cold, "D6", leaf),
            cold.baseline_digest(leaf),
            "{leaf}"
        );
    }
    for leaf in ["conclusion/01", "conclusion/40", "conclusion/25"] {
        let own = prove::digest_for_code(leaf, code_of(leaf)).unwrap();
        assert_eq!(cold.baseline_digest(leaf), Some(own.as_str()), "{leaf}");
    }
}

#[test]
fn the_subject_binds_the_proof_implementation_and_keeps_delta_captures() {
    let material = prove::subject_material().unwrap();
    assert!(material.contains("fn predicate_from"));
    assert!(material.contains("sim-world-proof-view-v1"));
    assert!(material.contains("fn plan_record"));
    assert!(material.contains("fn explanation_value"));
    assert!(material.contains("fn count_datum"));
    assert!(material.contains("name = \"sim-lib-world\""));
    assert!(material.contains("fn attempt_leaf"));
    assert!(material.contains("pub fn execute_proof_leaf"));
    assert!(material.contains(prove::proof_implementation()));
    let cold = prove::execute_cold().unwrap();
    let capture = prove::delta_capture(&cold, "D2", "conclusion/01").unwrap();
    assert!(capture.contains("edited private implementation"));
    let view = prove::cold_view(&cold).unwrap();
    let opened = prove::open_view(&view).unwrap().expect("view");
    assert_eq!(opened.captures, cold.captures);
    assert_eq!(
        prove::delta_capture(&opened, "D2", "conclusion/01"),
        Some(capture)
    );
    let boundary = prove::attempt_leaf("conclusion/01", "D1").unwrap();
    assert_eq!(boundary.attempts, 1);
    assert_eq!(boundary.effects, 0);
    assert_eq!(boundary.subprocesses, 0);
    assert_eq!(boundary.journal_records, 0);
}

#[test]
fn a_catalog_command_uses_the_qualified_route() {
    let route = prove::run_catalog_command().unwrap();
    assert!(route.scripted.contains("scripted process receipt"));
    assert_eq!(route.scripted_calls, 0);
    assert!(route.passed);
    assert_eq!(route.executed_calls, 1);
    assert_eq!(route.effects, 1);
    assert_eq!(route.subprocesses, 1);
    assert_eq!(route.journal_records, 2);
}

#[test]
fn a_scripted_command_receipt_cannot_enter_the_denominator() {
    let scripted = prove::admit_command("conclusion/01", true).unwrap_err();
    assert!(scripted.to_string().contains("scripted process receipt"));
    let plain = prove::admit_command("conclusion/01", false).unwrap_err();
    assert!(plain.to_string().contains("requires no owner command"));
    let cold = prove::execute_cold().unwrap();
    assert_eq!(cold.owner_commands, 0);
    assert!(cold.failures.is_empty());
    assert!(
        cold.leaves
            .iter()
            .all(|leaf| leaf.starts_with("conclusion/"))
    );
}

#[test]
fn a_failed_predicate_stays_in_the_denominator_and_opens_a_gap() {
    let cold = prove::execute_disturbed(prove::Disturbance {
        empty_fact: Some("stable/40".to_owned()),
        ..prove::Disturbance::none()
    })
    .unwrap();
    assert_eq!(cold.leaves.len(), 40);
    assert_eq!(cold.failures, vec!["conclusion/40".to_owned()]);
    assert!(cold.baseline_digest("conclusion/01").is_some());
    assert!(cold.captures["conclusion/01"].contains("baseline stable/01"));
    let record = prove::run("cold", None).unwrap();
    assert_eq!(number_field(&record, "denominator"), 29);
    assert_eq!(number_field(&record, "executed"), 29);
    assert!(text_field(&record, "gaps").is_empty());
    let view = prove::cold_view(&cold).unwrap();
    let opened = prove::open_view(&view).unwrap().expect("assay view");
    assert_eq!(opened.failures, vec!["conclusion/40".to_owned()]);
    assert_eq!(
        text_field(&prove::frontier(Some(&view)).unwrap(), "reason"),
        "refresh-needed"
    );
}

fn hex_utf8(text: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn code_of(leaf: &str) -> &'static str {
    match leaf {
        "conclusion/23" | "conclusion/24" => {
            "pure-check proof-code-alpha binds the projected bytes"
        }
        "conclusion/25" | "conclusion/26" => "pure-check checker-alpha binds the projected bytes",
        _ => "pure-check semantic text is canonical and non-empty",
    }
}
