// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::deliver;
use crate::service;

const DISCLOSURE: &str = "policy/no-v3-disclosure";

fn node<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
    match value {
        Datum::Node {
            tag: actual,
            fields,
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

fn field_opt<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

fn text(value: &Datum) -> &str {
    match value {
        Datum::String(value) => value,
        other => panic!("string, got {other:?}"),
    }
}

fn text_at<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a str {
    text(field(fields, name))
}

fn text_opt<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a str> {
    field_opt(fields, name).map(text)
}

fn number(value: &Datum) -> u64 {
    match value {
        Datum::Number(number) => number.canonical.parse().expect("count"),
        other => panic!("number, got {other:?}"),
    }
}

fn vector<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a [Datum] {
    match field(fields, name) {
        Datum::Vector(items) => items,
        other => panic!("{name}, got {other:?}"),
    }
}

fn snapshot_row(realization: &str, selector: &str) -> Vec<(Symbol, Datum)> {
    let value = service::snapshot(realization, selector).expect("snapshot");
    let fields = node(&value, "world/service-snapshot-v1");
    let observations = match field(fields, "observations") {
        Datum::Vector(items) => items,
        other => panic!("observations, got {other:?}"),
    };
    node(&observations[0], "world/service-observation-v1").to_vec()
}

fn deliver_fields(plan: &Datum) -> &[(Symbol, Datum)] {
    node(plan, "world/deliver-v1")
}

fn named<'a>(items: &'a [Datum], tag: &str, selector: &str) -> Option<&'a [(Symbol, Datum)]> {
    items.iter().find_map(|item| {
        let row = node(item, tag);
        (text_at(row, "selector") == selector).then_some(row)
    })
}

fn expected_cause(reason: &str, state: &str, kind: &str) -> &'static str {
    match (reason, state, kind) {
        ("completed", "current", _) => "already-true",
        ("desired-differs", "current", "tag") => "wrong-tag",
        ("desired-differs", "current", _) => "divergent",
        ("original-preserved", "lagging", _) => "registry-lag",
        ("original-preserved", "stale", _) => "stale-observation",
        ("original-preserved", "denied", _) => "denied-access",
        ("original-preserved", "partial", _) => "partial-success",
        ("original-preserved", "uncertain", _) => "uncertain-reading",
        ("original-preserved", "unavailable" | "absent", _) => "unavailable-receiver",
        other => panic!("no delivery cause for {other:?}"),
    }
}

fn expected_class(cause: &str) -> &'static str {
    match cause {
        "already-true" => "already-true",
        "divergent" | "wrong-tag" => "diverged",
        _ => "uncertain",
    }
}

fn expected_dispatches(cause: &str) -> u64 {
    match cause {
        "divergent" | "wrong-tag" | "partial-success" => 1,
        _ => 0,
    }
}

fn expected_ack(cause: &str) -> &'static str {
    match cause {
        "already-true" => "omitted",
        "divergent" | "wrong-tag" | "partial-success" => "missing",
        _ => "unsent",
    }
}

fn assert_row(row: &[(Symbol, Datum)], cause: &str) {
    let dispatches = expected_dispatches(cause);
    assert_eq!(text_at(row, "cause"), cause);
    assert_eq!(text_at(row, "class"), expected_class(cause));
    assert_eq!(text_at(row, "acknowledgement"), expected_ack(cause));
    assert_eq!(number(field(row, "dispatches")), dispatches);
    assert_eq!(number(field(row, "performer-calls")), dispatches);
    assert_eq!(number(field(row, "effects")), 0);
    assert_eq!(number(field(row, "subprocesses")), 0);
    let observations = number(field(row, "observations"));
    let minimum = if dispatches == 0 { 2 } else { 3 };
    assert!(
        observations >= minimum,
        "recovery observations {observations} for {cause}"
    );
}

#[test]
fn delivery_qualifies_each_release_row_from_the_live_service() {
    let probe = "git/branch/main";
    let before = service::snapshot("model", probe).expect("before");
    let plan = deliver::catalog("model").expect("deliver");
    assert_eq!(plan, deliver::catalog("model").expect("repeat"));
    let after = service::snapshot("model", probe).expect("after");
    assert_eq!(before, after);

    let release = crate::release::plan_catalog("model").expect("release");
    let release_fields = node(&release, "world/release-plan-v1");
    let fields = deliver_fields(&plan);
    assert_eq!(number(field(fields, "effects")), 0);
    assert_eq!(number(field(fields, "subprocesses")), 0);
    assert_eq!(field(fields, "resident"), &Datum::Bool(true));
    assert_eq!(field(fields, "recovered"), &Datum::Bool(true));
    assert_eq!(text_at(fields, "disclosure"), DISCLOSURE);
    assert_eq!(text_at(fields, "realization"), "model");
    assert_eq!(
        field(fields, "blocked"),
        &Datum::Bool(!vector(release_fields, "actions").is_empty())
    );
    assert_eq!(
        field(fields, "noop"),
        &Datum::Bool(vector(release_fields, "actions").is_empty())
    );

    let mut saw = BTreeSet::new();
    let mut dispatches = 0;
    let mut performer_calls = 0;
    let mut observations = 0;
    for item in vector(release_fields, "omitted") {
        let omission = node(item, "world/release-omission-v1");
        let selector = text_at(omission, "selector");
        let reading = snapshot_row("model", selector);
        assert_eq!(text_at(&reading, "state"), "current");
        let row = named(
            vector(fields, "omitted"),
            "world/deliver-omission-v1",
            selector,
        )
        .expect("omission");
        let cause = expected_cause(
            text_at(omission, "reason"),
            text_at(&reading, "state"),
            text_at(&reading, "kind"),
        );
        assert_row(row, cause);
        assert_eq!(text_at(row, "desired"), text_at(&reading, "value"));
        assert_eq!(text_at(row, "state"), text_at(&reading, "state"));
        assert_eq!(
            text_at(row, "intent"),
            format!("intent/reconcile/{selector}")
        );
        assert!(
            named(
                vector(fields, "actions"),
                "world/deliver-action-v1",
                selector
            )
            .is_none()
        );
        saw.insert(cause);
        dispatches += number(field(row, "dispatches"));
        performer_calls += number(field(row, "performer-calls"));
        observations += number(field(row, "observations"));
    }
    for item in vector(release_fields, "actions") {
        let action = node(item, "world/release-action-v1");
        let selector = text_at(action, "selector");
        let reading = snapshot_row("model", selector);
        let row = named(
            vector(fields, "actions"),
            "world/deliver-action-v1",
            selector,
        )
        .expect("action");
        let cause = expected_cause(
            text_at(action, "reason"),
            text_at(&reading, "state"),
            text_at(&reading, "kind"),
        );
        assert_row(row, cause);
        assert_eq!(text_at(row, "desired"), text_at(action, "desired"));
        assert_eq!(text_at(row, "state"), text_at(&reading, "state"));
        assert_eq!(text_at(row, "state"), text_at(action, "state"));
        assert_eq!(text_opt(row, "value"), text_opt(&reading, "value"));
        assert_eq!(text_at(row, "provenance"), text_at(&reading, "provenance"));
        assert_eq!(
            text_at(row, "intent"),
            format!("intent/reconcile/{selector}")
        );
        saw.insert(cause);
        dispatches += number(field(row, "dispatches"));
        performer_calls += number(field(row, "performer-calls"));
        observations += number(field(row, "observations"));
    }
    for cause in [
        "already-true",
        "divergent",
        "registry-lag",
        "stale-observation",
        "denied-access",
        "partial-success",
        "uncertain-reading",
    ] {
        assert!(saw.contains(cause), "missing {cause}");
    }
    assert!(!saw.contains("wrong-tag"));
    assert_eq!(number(field(fields, "dispatches")), dispatches);
    assert_eq!(number(field(fields, "performer-calls")), performer_calls);
    assert_eq!(dispatches, performer_calls);
    assert_eq!(number(field(fields, "observations")), observations);
    let rendered = render(&plan);
    assert!(rendered.contains("mirror/sim-private"));
    assert!(!rendered.contains("sim-private/"));
    assert!(!rendered.contains("bo-private"));
    assert!(!rendered.contains("docs/private"));
    assert!(!rendered.contains("/home/"));
    assert!(!rendered.contains("private-material:"));
}

#[test]
fn wrong_tag_blocks_and_partial_success_does_not_repeat() {
    let release = crate::release::plan_catalog("model").expect("release");
    let release_fields = node(&release, "world/release-plan-v1");
    let mut tag = None;
    for item in vector(release_fields, "omitted") {
        let selector = text_at(node(item, "world/release-omission-v1"), "selector");
        let reading = snapshot_row("model", selector);
        if text_at(&reading, "kind") == "tag" && text_at(&reading, "state") == "current" {
            tag = Some((selector.to_owned(), text_at(&reading, "value").to_owned()));
            break;
        }
    }
    let (selector, value) = tag.expect("current tag");
    let desired = format!("{value}-other");
    let plan = deliver::one("model", &selector, &desired).expect("wrong tag");
    assert_eq!(
        plan,
        deliver::one("model", &selector, &desired).expect("repeat")
    );
    let fields = deliver_fields(&plan);
    assert_eq!(field(fields, "blocked"), &Datum::Bool(true));
    assert_eq!(field(fields, "noop"), &Datum::Bool(false));
    assert!(vector(fields, "omitted").is_empty());
    let row = named(
        vector(fields, "actions"),
        "world/deliver-action-v1",
        &selector,
    )
    .expect("tag action");
    let reading = snapshot_row("model", &selector);
    let cause = expected_cause(
        "desired-differs",
        text_at(&reading, "state"),
        text_at(&reading, "kind"),
    );
    assert_eq!(cause, "wrong-tag");
    assert_row(row, cause);
    assert_eq!(text_at(row, "desired"), desired);
    assert_eq!(number(field(fields, "dispatches")), 1);
    assert_eq!(number(field(fields, "performer-calls")), 1);
    assert_eq!(number(field(fields, "effects")), 0);

    let mut partial = None;
    for item in vector(release_fields, "actions") {
        let action = node(item, "world/release-action-v1");
        let selector = text_at(action, "selector");
        let reading = snapshot_row("model", selector);
        if text_at(&reading, "state") == "partial" {
            partial = Some((selector.to_owned(), text_at(action, "desired").to_owned()));
            break;
        }
    }
    let (selector, desired) = partial.expect("partial service");
    let plan = deliver::one("model", &selector, &desired).expect("partial");
    let row = named(
        vector(deliver_fields(&plan), "actions"),
        "world/deliver-action-v1",
        &selector,
    )
    .expect("partial action");
    let reading = snapshot_row("model", &selector);
    let cause = expected_cause(
        "original-preserved",
        text_at(&reading, "state"),
        text_at(&reading, "kind"),
    );
    assert_eq!(cause, "partial-success");
    assert_row(row, cause);
    assert_eq!(number(field(deliver_fields(&plan), "effects")), 0);
    assert_eq!(number(field(deliver_fields(&plan), "dispatches")), 1);
}

#[test]
fn delivery_status_and_why_read_the_qualification() {
    let plan = deliver::catalog("model").expect("plan");
    let fields = deliver_fields(&plan);
    let status = deliver::status("model").expect("status");
    let status_fields = node(&status, "world/deliver-status-v1");
    assert_eq!(field(status_fields, "noop"), field(fields, "noop"));
    assert_eq!(field(status_fields, "blocked"), field(fields, "blocked"));
    assert_eq!(field(status_fields, "omitted"), field(fields, "omitted"));
    assert_eq!(
        number(field(status_fields, "dispatches")),
        number(field(fields, "dispatches"))
    );
    assert_eq!(
        number(field(status_fields, "performer-calls")),
        number(field(fields, "performer-calls"))
    );
    assert_eq!(number(field(status_fields, "effects")), 0);
    assert_eq!(number(field(status_fields, "subprocesses")), 0);
    assert_eq!(text_at(status_fields, "disclosure"), DISCLOSURE);
    let actions = vector(fields, "actions");
    let remaining = vector(status_fields, "remaining");
    assert_eq!(actions.len(), remaining.len());
    for (action, remain) in actions.iter().zip(remaining.iter()) {
        let action = node(action, "world/deliver-action-v1");
        let remain = node(remain, "world/deliver-remaining-v1");
        assert_eq!(text_at(remain, "selector"), text_at(action, "selector"));
        assert_eq!(text_at(remain, "cause"), text_at(action, "cause"));
        assert_eq!(text_at(remain, "intent"), text_at(action, "intent"));
    }
    let omissions = vector(fields, "omitted");
    assert!(!omissions.is_empty());
    let done = text_at(node(&omissions[0], "world/deliver-omission-v1"), "selector").to_owned();
    let why_done_value = deliver::why("model", &done).expect("why omitted");
    let why_done = node(&why_done_value, "world/deliver-why-v1");
    assert_eq!(text_at(why_done, "cause"), "already-true");
    assert_eq!(text_at(why_done, "class"), "already-true");
    assert_eq!(number(field(why_done, "dispatches")), 0);
    assert_eq!(number(field(why_done, "effects")), 0);
    let selector = text_at(node(&actions[0], "world/deliver-action-v1"), "selector").to_owned();
    let action = node(&actions[0], "world/deliver-action-v1");
    let why_left_value = deliver::why("model", &selector).expect("why action");
    let why_left = node(&why_left_value, "world/deliver-why-v1");
    assert_eq!(text_at(why_left, "cause"), text_at(action, "cause"));
    assert_eq!(text_at(why_left, "class"), text_at(action, "class"));
    assert_eq!(text_at(why_left, "state"), text_at(action, "state"));
    assert_eq!(
        number(field(why_left, "dispatches")),
        number(field(action, "dispatches"))
    );
    assert_eq!(number(field(why_left, "effects")), 0);

    let absent = deliver::why("model", "git/branch/missing").expect("absent");
    assert_eq!(
        text_at(node(&absent, "world/deliver-gap-v1"), "reason"),
        "selector-absent"
    );
    let empty = deliver::why("model", "").expect("empty");
    assert_eq!(
        text_at(node(&empty, "world/deliver-gap-v1"), "reason"),
        "empty-selector"
    );
    let host = deliver::status("host").expect("host");
    assert_eq!(
        text_at(node(&host, "world/deliver-gap-v1"), "reason"),
        "unknown-realization"
    );
}

#[test]
fn delivery_refuses_private_material_and_unavailable_receivers() {
    let leaked =
        deliver::one("model", "git/branch/main", "private-material:s3cret-token").expect("gap");
    assert_eq!(
        text_at(node(&leaked, "world/deliver-gap-v1"), "reason"),
        "private-leakage"
    );
    assert_eq!(
        text_at(node(&leaked, "world/deliver-gap-v1"), "selector"),
        ""
    );
    assert!(!render(&leaked).contains("s3cret-token"));
    assert!(!render(&leaked).contains("private-material:"));
    let hidden = deliver::why("model", "/home/secret-path").expect("home");
    assert!(!render(&hidden).contains("/home/"));
    assert_eq!(
        text_at(node(&hidden, "world/deliver-gap-v1"), "reason"),
        "private-leakage"
    );
    let hidden_name = deliver::catalog("private-material:s3cret-token").expect("name");
    assert!(!render(&hidden_name).contains("s3cret-token"));
    assert_eq!(
        text_at(node(&hidden_name, "world/deliver-gap-v1"), "realization"),
        ""
    );

    let kept = deliver::one("model", "mirror/sim-private", "model-commit-main").expect("kept");
    let action = named(
        vector(deliver_fields(&kept), "actions"),
        "world/deliver-action-v1",
        "mirror/sim-private",
    )
    .expect("mirror");
    let reading = snapshot_row("model", "mirror/sim-private");
    let cause = expected_cause(
        "original-preserved",
        text_at(&reading, "state"),
        text_at(&reading, "kind"),
    );
    assert_eq!(cause, "denied-access");
    assert_row(action, cause);
    assert!(render(&kept).contains("mirror/sim-private"));
    assert!(!render(&kept).contains("sim-private/"));

    let token = text_at(&snapshot_row("model", "git/branch/main"), "value").to_owned();
    for realization in ["native", "real"] {
        let plan = deliver::catalog(realization).expect("unavailable");
        let rendered = render(&plan);
        assert!(!rendered.contains("model/"));
        let fields = deliver_fields(&plan);
        assert_eq!(number(field(fields, "effects")), 0);
        assert_eq!(number(field(fields, "subprocesses")), 0);
        assert_eq!(number(field(fields, "dispatches")), 0);
        assert_eq!(number(field(fields, "performer-calls")), 0);
        assert!(vector(fields, "omitted").is_empty());
        assert!(!vector(fields, "actions").is_empty());
        for action in vector(fields, "actions") {
            let row = node(action, "world/deliver-action-v1");
            let reading = snapshot_row(realization, text_at(row, "selector"));
            let cause = expected_cause(
                "original-preserved",
                text_at(&reading, "state"),
                text_at(&reading, "kind"),
            );
            assert_eq!(cause, "unavailable-receiver");
            assert_row(row, cause);
            assert!(text_opt(row, "value").is_none());
            assert!(!text_at(row, "provenance").contains("model/"));
        }
    }
    let host = deliver::catalog("host").expect("host");
    assert!(!render(&host).contains(&token));
    assert!(!render(&host).contains("model/"));
    assert_eq!(
        text_at(node(&host, "world/deliver-gap-v1"), "reason"),
        "unknown-realization"
    );
}
