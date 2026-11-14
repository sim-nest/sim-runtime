// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use crate::prove::{self, graph_choice};
use crate::prove_command::ProveCommandLib;

#[test]
fn loaded_proof_view_compares_the_cold_denominator() {
    let prove = ProveCommandLib::new();
    let exported = prove
        .run(&args(["prove", "run", "proof"]))
        .expect("proof export");
    assert_eq!(tag(&exported), "prove/proof-v1");
    let view = text(&exported, "view");
    assert!(view.starts_with("sim-world-proof-view-v1"));
    let opened = prove::open_view(&view).unwrap().expect("admitted proof");
    assert_eq!(opened.leaves.len(), 40);

    let d1_report = prove::reuse(&opened, "D1", graph_choice)
        .unwrap()
        .expect("D1");
    let d2_report = prove::reuse(&opened, "D2", graph_choice)
        .unwrap()
        .expect("D2");
    assert_ne!(d2_report.basis, 10_000);
    assert_eq!(d1_report.false_reuse, 0);

    let d1 = prove
        .run(&with_view(&["prove", "run", "D1"], &view))
        .expect("D1 command");
    let d2 = prove
        .run(&with_view(&["prove", "run", "D2"], &view))
        .expect("D2 command");
    assert_eq!(tag(&d1), "prove/reuse-v1");
    assert_eq!(tag(&d2), "prove/reuse-v1");
    assert_eq!(number(&d1, "denominator"), 40);
    assert_eq!(number(&d2, "denominator"), 40);
    assert_eq!(
        number(&d1, "reuse-basis-points"),
        u64::from(d1_report.basis)
    );
    assert_eq!(number(&d1, "false-reuse"), d1_report.false_reuse);
    assert_eq!(
        number(&d2, "reuse-basis-points"),
        u64::from(d2_report.basis)
    );
    assert_eq!(number(&d2, "false-reuse"), d2_report.false_reuse);
    assert_eq!(number(&d2, "reuse-requests"), d2_report.reuse_requests);
    assert_eq!(text(&d2, "rerun"), joined(&d2_report.rerun));
    assert_eq!(text(&d1, "rerun"), joined(&d1_report.rerun));

    let why_rerun = prove
        .run(&with_view(&["prove", "why", "D2", "conclusion/01"], &view))
        .expect("why rerun");
    assert!(!bool_field(&why_rerun, "reused"));
    assert!(!text(&why_rerun, "exercised").is_empty());
    let why_kept = prove
        .run(&with_view(&["prove", "why", "D2", "conclusion/40"], &view))
        .expect("why kept");
    assert!(bool_field(&why_kept, "reused"));
    assert!(text(&why_kept, "exercised").is_empty());
    assert!(!bool_field(&why_kept, "false-reuse"));

    let retained = insert_record(&view, "delta D2 ", "retain conclusion/01");
    let forced = prove
        .run(&with_view(&["prove", "run", "D2"], &retained))
        .expect("retained D2");
    assert!(number(&forced, "false-reuse") > number(&d2, "false-reuse"));
    assert!(
        text(&forced, "reused-leaves")
            .split(' ')
            .any(|leaf| leaf == "conclusion/01")
    );
    assert!(
        !text(&forced, "rerun")
            .split(' ')
            .any(|leaf| leaf == "conclusion/01")
    );

    let claimed = insert_record(
        &view,
        "delta D1 ",
        "claim conclusion/40 change/private-implementation",
    );
    let unread = prove
        .run(&with_view(&["prove", "run", "D1"], &claimed))
        .expect("claimed D1");
    assert!(number(&unread, "false-reuse") > number(&d1, "false-reuse"));
    assert_eq!(
        number(&unread, "reuse-basis-points"),
        number(&d1, "reuse-basis-points")
    );

    let mut tampered = view.clone();
    let last = tampered.pop().expect("view char");
    tampered.push(if last == 'a' { 'b' } else { 'a' });
    let stale = prove
        .run(&with_view(&["prove", "run", "D2"], &tampered))
        .expect("tampered");
    assert_eq!(text(&stale, "reason"), "refresh-needed");

    let frontier = prove
        .run(&with_view(&["prove", "frontier"], &view))
        .expect("frontier");
    assert_eq!(tag(&frontier), "prove/frontier-v1");
    assert!(bool_field(&frontier, "complete"));
    assert_eq!(number(&frontier, "denominator"), 40);
    let status = prove
        .run(&with_view(&["prove", "status"], &view))
        .expect("status");
    assert_eq!(tag(&status), "prove/status-v1");
    assert_eq!(number(&status, "denominator"), 40);
    assert!(bool_field(&status, "resident"));
}

fn insert_record(view: &str, marker_prefix: &str, record: &str) -> String {
    let marker = view
        .split(';')
        .find(|part| part.starts_with(marker_prefix))
        .expect("marker")
        .to_owned();
    prove::reseal_replaced(view, &marker, &format!("{record};{marker}"))
        .unwrap()
        .expect("reseal")
}

fn args(parts: [&str; 3]) -> Vec<String> {
    parts.into_iter().map(str::to_owned).collect()
}

fn with_view(head: &[&str], view: &str) -> Vec<String> {
    head.iter()
        .copied()
        .map(str::to_owned)
        .chain(std::iter::once(view.to_owned()))
        .collect()
}

fn joined(values: &std::collections::BTreeSet<String>) -> String {
    values.iter().cloned().collect::<Vec<_>>().join(" ")
}

fn tag(value: &Datum) -> String {
    let Datum::Node { tag, .. } = value else {
        panic!("node");
    };
    tag.to_string()
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
