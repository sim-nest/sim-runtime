// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use crate::prove;

#[test]
fn the_public_denominator_is_the_phase_gates() {
    let cold = prove::run("cold", None).unwrap();
    assert_eq!(number(&cold, "denominator"), 29);
    assert_eq!(number(&cold, "executed"), 3);
    assert_eq!(number(&cold, "owner-commands"), 0);
    assert_eq!(number(&cold, "requests"), 3);
    let gaps = text(&cold, "gaps");
    assert!(gaps.contains("gap phase/00 no-owner-result"));
    assert!(!gaps.contains("phase/06"));
    assert!(gaps.contains("gap phase/10 host-read"));
    assert!(gaps.contains("gap phase/11 foreign-crate"));
    assert!(gaps.contains("gap phase/14 circular"));
    assert!(gaps.contains("gap phase/27 held"));
    assert!(gaps.contains("gap phase/28 unimplemented"));
    assert!(gaps.contains("gap phase/02 same-computation"));
    assert!(!gaps.contains("phase/07"));
    assert!(!gaps.contains("phase/08"));
    assert!(!gaps.contains("conclusion/"));
    assert!(text(&cold, "view").contains("sim-world-portable-union-v1"));
    let oracle = include_str!("assay_oracle.rs");
    assert!(!oracle.contains("portable/cold-proof/"));
    assert!(oracle.contains("assay/conclusion/01"));
}

#[test]
fn reuse_of_owner_leaves_is_complete_and_the_frontier_stays_open() {
    let cold = prove::run("cold", None).unwrap();
    let view = text(&cold, "view");
    let d1 = prove::run("D1", Some(&view)).unwrap();
    assert_eq!(number(&d1, "denominator"), 29);
    assert_eq!(number(&d1, "executed"), 3);
    assert_eq!(number(&d1, "reused"), 3);
    assert_eq!(number(&d1, "false-reuse"), 0);
    assert_eq!(number(&d1, "reuse-basis-points"), 10_000);
    assert_eq!(text(&d1, "rerun"), "");
    let frontier = prove::frontier(Some(&view)).unwrap();
    assert!(!bool_field(&frontier, "complete"));
    assert_eq!(number(&frontier, "executed"), number(&cold, "executed"));
    let why = prove::why("D2", "phase/07", Some(&view)).unwrap();
    assert!(bool_field(&why, "reused"));
    let project = prove::why("D2", "phase/06", Some(&view)).unwrap();
    assert!(bool_field(&project, "reused"));
    let plan = prove::why("D2", "phase/08", Some(&view)).unwrap();
    assert!(bool_field(&plan, "reused"));
    assert!(!text(&cold, "view").contains("gap phase/06"));
    assert_eq!(text(&why, "exercised"), "");
}

#[test]
fn the_plan_lists_every_phase_and_does_not_drop_a_gate() {
    let mut token: Option<String> = None;
    let mut seen = Vec::new();
    for _ in 0..6 {
        let page = match token.as_deref() {
            None => prove::plan(None).unwrap(),
            Some(token) => prove::resume(token, None).unwrap(),
        };
        assert_eq!(number(&page, "denominator"), 29);
        let leaves = text(&page, "leaves");
        if !leaves.is_empty() {
            seen.extend(leaves.split(' ').map(str::to_owned));
        }
        match continuation(&page) {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    assert_eq!(seen.len(), 29);
    assert_eq!(seen[0], "phase/00");
    assert_eq!(seen[28], "phase/28");
    assert!(!seen.iter().any(|leaf| leaf.contains("linux")));
}

fn continuation(value: &Datum) -> Option<String> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields.iter().find_map(|(name, field)| {
        if name.to_string() != "continuation" {
            return None;
        }
        match field {
            Datum::String(text) => Some(text.clone()),
            _ => None,
        }
    })
}

fn text(value: &Datum, name: &str) -> String {
    let Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    match field {
        Datum::String(text) => text.clone(),
        Datum::Vector(items) => items
            .iter()
            .map(|item| match item {
                Datum::String(text) => text.as_str(),
                other => panic!("string item, got {other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => panic!("text field {name}: {other:?}"),
    }
}

fn number(value: &Datum, name: &str) -> u64 {
    let Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    let Datum::Number(number) = field else {
        panic!("number {name}");
    };
    number
        .canonical
        .parse()
        .unwrap_or_else(|_| panic!("{name}"))
}

fn bool_field(value: &Datum, name: &str) -> bool {
    let Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    let Some((_, field)) = fields.iter().find(|(field, _)| field.to_string() == name) else {
        panic!("missing {name}");
    };
    let Datum::Bool(value) = field else {
        panic!("bool {name}");
    };
    *value
}
