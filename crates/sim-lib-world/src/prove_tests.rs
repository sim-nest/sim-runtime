// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;

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
    assert_eq!(cold.requests, edges * 7);
    assert_eq!(cold.edges, edges);
    assert_ne!(
        cold.baseline_digest("conclusion/01"),
        cold.baseline_digest("conclusion/03")
    );
    let record = prove::run("cold").unwrap();
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
    let missing = prove::frontier_leaf("leaf/missing-portable").unwrap();
    assert_eq!(text_field(&missing, "reason"), "unavailable-proof");
    let native = prove::frontier_leaf("leaf/native-worker-adapter").unwrap();
    assert_eq!(text_field(&native, "reason"), "excluded-scope");
    let assay = prove::frontier_leaf("leaf/native-assay").unwrap();
    assert_eq!(text_field(&assay, "reason"), "excluded-scope");
    let unknown = prove::run("D7").unwrap();
    assert_eq!(text_field(&unknown, "reason"), "unknown-delta");
}

#[test]
fn plan_resume_advances_one_page_and_a_bad_token_is_a_gap() {
    let first = prove::plan(None).unwrap();
    assert_eq!(tag(&first), "prove/plan-v1");
    let token = text_field(&first, "continuation").to_owned();
    assert_eq!(token.len(), 64);
    let second = prove::resume(&token).unwrap();
    assert_eq!(tag(&second), "prove/plan-v1");
    assert_ne!(text_field(&first, "leaves"), text_field(&second, "leaves"));
    let stale = prove::resume("stale").unwrap();
    assert_eq!(text_field(&stale, "reason"), "stale-or-missing");
    let zeros = "0".repeat(64);
    let wrong = prove::resume(&zeros).unwrap();
    assert_eq!(text_field(&wrong, "reason"), "stale-or-missing");
}

#[test]
fn frontier_of_the_cold_run_is_empty_and_resume_repeats_it() {
    let frontier = prove::frontier().unwrap();
    assert_eq!(tag(&frontier), "prove/frontier-v1");
    assert_eq!(number_field(&frontier, "executed"), 40);
    assert!(frontier_failures_empty(&frontier));
    let token = text_field(&frontier, "continuation").to_owned();
    let again = prove::resume(&token).unwrap();
    assert_eq!(tag(&again), "prove/frontier-v1");
    assert!(frontier_failures_empty(&again));
}

#[test]
fn cached_status_and_why_stay_inside_the_ceiling() {
    let status = prove::status().unwrap();
    assert_eq!(number_field(&status, "requests"), 0);
    assert_eq!(number_field(&status, "denominator"), 40);
    for command in ["status", "why"] {
        let measured = prove::measure(command).unwrap();
        assert!(measured.within_ceiling, "{command}");
        assert_eq!(number_field(&measured.record, "samples"), 200);
        assert_eq!(number_field(&measured.record, "records"), 100_000);
    }
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

fn frontier_failures_empty(value: &sim_kernel::Datum) -> bool {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        return false;
    };
    fields.iter().any(|(name, field)| {
        name.to_string() == "failures"
            && matches!(field, sim_kernel::Datum::Vector(items) if items.is_empty())
            && fields.iter().any(|(name, field)| {
                name.to_string() == "complete" && matches!(field, sim_kernel::Datum::Bool(true))
            })
    })
}
