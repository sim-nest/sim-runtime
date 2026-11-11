// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use crate::prove;

#[test]
fn the_public_denominator_is_the_phase_gates() {
    let before = crate::journal::calls_on_this_thread();
    let operate_before = crate::operate::calls_on_this_thread();
    let cold = prove::run("cold", None).unwrap();
    assert_eq!(
        crate::journal::calls_on_this_thread(),
        before.saturating_add(2)
    );
    assert_eq!(
        crate::operate::calls_on_this_thread(),
        operate_before.saturating_add(4)
    );
    assert_eq!(number(&cold, "denominator"), 29);
    assert_eq!(number(&cold, "executed"), 21);
    assert_eq!(number(&cold, "owner-commands"), 0);
    assert_eq!(number(&cold, "requests"), 21);
    let gaps = text(&cold, "gaps");
    assert!(gaps.contains("gap phase/00 no-owner-result"));
    assert!(!gaps.contains("phase/06"));
    assert!(gaps.contains("gap phase/10 host-read"));
    assert!(gaps.contains("gap phase/11 foreign-crate"));
    assert!(gaps.contains("gap phase/14 circular"));
    assert!(gaps.contains("gap phase/27 held"));
    assert!(gaps.contains("gap phase/28 unimplemented"));
    assert!(!gaps.contains("phase/02"));
    assert!(!gaps.contains("phase/07"));
    assert!(!gaps.contains("phase/08"));
    assert!(!gaps.contains("phase/09"));
    assert!(!gaps.contains("phase/19"));
    assert!(!gaps.contains("phase/20"));
    assert!(!gaps.contains("phase/21"));
    assert!(!gaps.contains("phase/22"));
    assert!(!gaps.contains("phase/23"));
    assert!(!gaps.contains("phase/24"));
    assert!(!gaps.contains("phase/25"));
    assert!(!gaps.contains("phase/13"));
    assert!(!gaps.contains("phase/15"));
    assert!(!gaps.contains("phase/16"));
    assert!(!gaps.contains("phase/17"));
    assert!(!gaps.contains("phase/18"));
    assert!(!gaps.contains("phase/05"));
    assert!(!gaps.contains("phase/01"));
    assert!(!gaps.contains("phase/04"));
    assert!(!gaps.contains("phase/26"));
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
    assert_eq!(number(&d1, "executed"), 21);
    assert_eq!(number(&d1, "reused"), 21);
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
    let observed = prove::why("D2", "phase/09", Some(&view)).unwrap();
    assert!(bool_field(&observed, "reused"));
    let service = prove::why("D2", "phase/19", Some(&view)).unwrap();
    assert!(bool_field(&service, "reused"));
    let release = prove::why("D2", "phase/20", Some(&view)).unwrap();
    assert!(bool_field(&release, "reused"));
    let deliver = prove::why("D2", "phase/21", Some(&view)).unwrap();
    assert!(bool_field(&deliver, "reused"));
    let canary = prove::why("D2", "phase/22", Some(&view)).unwrap();
    assert!(bool_field(&canary, "reused"));
    let work = prove::why("D2", "phase/23", Some(&view)).unwrap();
    assert!(bool_field(&work, "reused"));
    let author = prove::why("D2", "phase/24", Some(&view)).unwrap();
    assert!(bool_field(&author, "reused"));
    let conduct = prove::why("D2", "phase/25", Some(&view)).unwrap();
    assert!(bool_field(&conduct, "reused"));
    let schedule = prove::why("D2", "phase/13", Some(&view)).unwrap();
    assert!(bool_field(&schedule, "reused"));
    let converge = prove::why("D2", "phase/15", Some(&view)).unwrap();
    assert!(bool_field(&converge, "reused"));
    let fixed = prove::why("D2", "phase/16", Some(&view)).unwrap();
    assert!(bool_field(&fixed, "reused"));
    let transact = prove::why("D2", "phase/17", Some(&view)).unwrap();
    assert!(bool_field(&transact, "reused"));
    let land = prove::why("D2", "phase/18", Some(&view)).unwrap();
    assert!(bool_field(&land, "reused"));
    let recovered = prove::why("D2", "phase/05", Some(&view)).unwrap();
    assert!(bool_field(&recovered, "reused"));
    let bound = prove::why("D2", "phase/01", Some(&view)).unwrap();
    assert!(bool_field(&bound, "reused"));
    let operated = prove::why("D2", "phase/04", Some(&view)).unwrap();
    assert!(bool_field(&operated, "reused"));
    let journal = prove::why("D2", "phase/02", Some(&view)).unwrap();
    assert!(bool_field(&journal, "reused"));
    let qualify = prove::why("D2", "phase/26", Some(&view)).unwrap();
    assert!(bool_field(&qualify, "reused"));
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
