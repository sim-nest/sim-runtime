// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::{BTreeSet, HashSet};

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::plan::{self, Leaf, View};
use crate::query::{self, SAMPLES, WARMUPS};

const PLAN_KIND: &str = "world/proof-plan-v1";

fn node<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
    match value {
        Datum::Node {
            tag: actual,
            fields,
            ..
        } if actual.as_qualified_str() == tag => fields,
        other => panic!("expected {tag}, got {other:?}"),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a Datum {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn vector(value: &Datum) -> &[Datum] {
    match value {
        Datum::Vector(items) => items,
        other => panic!("vector, got {other:?}"),
    }
}

fn texts(value: &Datum) -> Vec<String> {
    match value {
        Datum::Vector(items) => items
            .iter()
            .map(|item| match item {
                Datum::String(text) => text.clone(),
                other => panic!("string, got {other:?}"),
            })
            .collect(),
        other => panic!("vector, got {other:?}"),
    }
}

fn text(value: &Datum) -> &str {
    match value {
        Datum::String(text) => text,
        other => panic!("string, got {other:?}"),
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[test]
fn repeated_equal_inputs_match_and_views_agree() {
    let first = plan::open(View::Expression, None).unwrap();
    let second = plan::open(View::Expression, None).unwrap();
    assert_eq!(render(&first), render(&second));
    let page = node(&first, PLAN_KIND);
    let world = text(field(page, "world")).to_owned();
    assert_eq!(world.len(), 64);
    assert!(world.chars().all(|ch| ch.is_ascii_hexdigit()));
    let text_value = plan::open(View::Text, None).unwrap();
    let card_value = plan::open(View::Card, None).unwrap();
    let text_page = node(&text_value, "world/proof-plan-text-v1");
    let card_page = node(&card_value, "world/proof-plan-card-v1");
    assert_eq!(text(field(text_page, "world")), world);
    assert_eq!(
        text(field(text_page, "affected")),
        texts(field(page, "affected")).join(" ")
    );
    assert_eq!(text(field(card_page, "core/subject")), world);
    assert_eq!(text(field(card_page, "core/kind")), PLAN_KIND);
    assert_eq!(
        text(field(text_page, "legal-ready")),
        text(field(card_page, "core/result"))
    );
    assert_eq!(
        text(field(text_page, "legal-ready")),
        texts(field(page, "legal-ready")).join(" ")
    );
    assert_eq!(
        text(field(text_page, "floor")),
        texts(field(page, "floor")).join(" ")
    );
    assert_eq!(
        text(field(text_page, "gaps")),
        text(field(card_page, "core/requires"))
    );
    assert_eq!(
        text(field(text_page, "continuation")),
        text(field(page, "continuation"))
    );
    assert_eq!(
        text(field(card_page, "core/args")),
        text(field(page, "continuation"))
    );
    assert_eq!(field(text_page, "page"), field(page, "page"));
    assert_eq!(field(page, "effects"), &query::count_datum(0));
    assert_eq!(field(page, "subprocesses"), &query::count_datum(0));
    let names: Vec<String> = card_page
        .iter()
        .map(|(symbol, _)| symbol.as_qualified_str())
        .collect();
    let expected: Vec<String> = sim_kernel::card::card_fixed_predicates()
        .iter()
        .map(Symbol::as_qualified_str)
        .collect();
    assert_eq!(names, expected);
    assert_ne!(render(&first), render(&text_value));
}

#[test]
fn every_leaf_has_a_reason_and_the_floor_stays_complete() {
    let opened = plan::open(View::Expression, None).unwrap();
    let first = node(&opened, PLAN_KIND);
    let leaves = match field(first, "leaves") {
        Datum::Vector(items) => items,
        other => panic!("leaves, got {other:?}"),
    };
    let mut reasons = BTreeSet::new();
    let mut previous_mass = usize::MAX;
    for id in texts(field(first, "legal-ready")) {
        let leaf = leaves
            .iter()
            .map(|item| node(item, "world/plan-leaf-v1"))
            .find(|leaf| text(field(leaf, "conclusion")) == id)
            .unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(field(leaf, "included"), &Datum::Bool(true));
        assert_eq!(text(field(leaf, "reason")), "legal-ready");
        let mass = texts(field(leaf, "facts")).len();
        assert!(mass <= previous_mass, "{id} inverted the mass order");
        assert!(mass > 0, "{id} is legal without a fact");
        previous_mass = mass;
        reasons.insert(text(field(leaf, "reason")).to_owned());
    }
    for item in leaves {
        let leaf = node(item, "world/plan-leaf-v1");
        let reason = text(field(leaf, "reason"));
        assert!(!reason.is_empty());
        reasons.insert(reason.to_owned());
    }
    for required in [
        "legal-ready",
        "unknown-owner",
        "revoked-evidence",
        "missing-source",
    ] {
        assert!(reasons.contains(required), "missing {required}");
    }
    let floor = texts(field(first, "floor"));
    assert_eq!(floor, texts(field(first, "requested")));
    let rows = match field(
        node(field(first, "budget"), "world/query-budget-v1"),
        "rows",
    ) {
        Datum::Number(number) => number.canonical.parse::<usize>().unwrap(),
        other => panic!("rows, got {other:?}"),
    };
    assert!(floor.len() > rows, "floor was truncated to the row budget");
    assert_eq!(texts(field(first, "affected")), Vec::<String>::new());
    let token = text(field(first, "continuation")).to_owned();
    let next = plan::open(View::Expression, Some(&token)).unwrap();
    let second = node(&next, PLAN_KIND);
    assert_eq!(texts(field(second, "floor")), floor);
    assert_eq!(texts(field(second, "affected")), Vec::<String>::new());
    assert_eq!(field(second, "gaps"), field(first, "gaps"));
    assert_eq!(text(field(second, "world")), text(field(first, "world")));
    assert_ne!(field(second, "explanations"), field(first, "explanations"));
    assert_eq!(vector(field(first, "explanations")).len(), rows);
}

#[test]
fn a_bad_token_is_a_gap_and_pages_do_not_mix() {
    for view in [View::Expression, View::Text, View::Card] {
        let opened = plan::open(view, Some("stale")).unwrap();
        let gap = node(&opened, "world/plan-gap-v1");
        assert_eq!(text(field(gap, "reason")), "stale-or-missing");
        assert!(gap.iter().all(|(name, _)| {
            !matches!(
                name.as_qualified_str().as_str(),
                "floor" | "legal-ready" | "explanations" | "leaves" | "closure"
            )
        }));
        assert_eq!(field(gap, "effects"), &query::count_datum(0));
        assert_eq!(field(gap, "subprocesses"), &query::count_datum(0));
    }
    let mut seen = HashSet::new();
    let mut token = String::new();
    let mut pages = 0usize;
    loop {
        let opened = plan::open(
            View::Expression,
            (!token.is_empty()).then_some(token.as_str()),
        )
        .unwrap();
        let page = node(&opened, PLAN_KIND);
        pages += 1;
        let explanations = vector(field(page, "explanations")).to_vec();
        for item in &explanations {
            let explanation = node(item, "world/explanation-v1");
            let key = format!(
                "{} {}",
                text(field(explanation, "conclusion")),
                text(field(explanation, "fact"))
            );
            assert!(seen.insert(key.clone()), "repeated explanation {key}");
            assert!(!key.contains("stranger"));
        }
        token = text(field(page, "continuation")).to_owned();
        if token.is_empty() {
            break;
        }
        assert!(pages < 8, "continuation did not end");
    }
    assert!(seen.iter().any(|key| key.contains("fact/source-gap")));
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 5);
    let opened = plan::open(View::Expression, Some("")).unwrap();
    let gap = node(&opened, "world/plan-gap-v1");
    assert_eq!(text(field(gap, "reason")), "stale-or-missing");
}

#[test]
fn equal_mass_orders_by_conclusion_id() {
    let heavier = Leaf {
        conclusion: "conclusion/m".to_owned(),
        facts: vec!["fact/a".to_owned(), "fact/b".to_owned()],
        evidence: None,
        included: true,
        reason: "legal-ready",
    };
    let early = Leaf {
        conclusion: "conclusion/a".to_owned(),
        facts: vec!["fact/a".to_owned()],
        evidence: None,
        included: true,
        reason: "legal-ready",
    };
    let late = Leaf {
        conclusion: "conclusion/z".to_owned(),
        facts: vec!["fact/z".to_owned()],
        evidence: None,
        included: true,
        reason: "legal-ready",
    };
    let mut leaves = [late, heavier, early];
    leaves.sort_by(plan::legal_order);
    let ids: Vec<&str> = leaves.iter().map(|leaf| leaf.conclusion.as_str()).collect();
    assert_eq!(ids, ["conclusion/m", "conclusion/a", "conclusion/z"]);
}

#[test]
fn measure_plan_records_zero_effects_inside_the_ceiling() {
    let measured = plan::measure().unwrap();
    assert!(measured.within_ceiling);
    let record = node(&measured.record, "world/query-latency-v1");
    assert_eq!(text(field(record, "command")), "plan");
    assert_eq!(field(record, "effects"), &query::count_datum(0));
    assert_eq!(field(record, "subprocesses"), &query::count_datum(0));
    assert_eq!(field(record, "resident"), &Datum::Bool(true));
    assert_eq!(
        field(record, "warmups"),
        &query::count_datum(count(WARMUPS))
    );
    assert_eq!(
        field(record, "samples"),
        &query::count_datum(count(SAMPLES))
    );
}
