// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::command::render;
use crate::release;
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

fn strings(value: &Datum) -> Vec<String> {
    match value {
        Datum::Vector(items) => items.iter().map(text).map(str::to_owned).collect(),
        other => panic!("vector, got {other:?}"),
    }
}

fn zero(value: &Datum) -> bool {
    matches!(value, Datum::Number(number) if number.canonical == "0")
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

fn vector<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a [Datum] {
    match field(fields, name) {
        Datum::Vector(items) => items,
        other => panic!("{name}, got {other:?}"),
    }
}

fn named<'a>(items: &'a [Datum], tag: &str, selector: &str) -> Option<&'a [(Symbol, Datum)]> {
    items.iter().find_map(|item| {
        let row = node(item, tag);
        (text_at(row, "selector") == selector).then_some(row)
    })
}

fn service_fields(plan: &Datum) -> &[(Symbol, Datum)] {
    node(plan, "world/service-plan-v1")
}

fn release_fields(plan: &Datum) -> &[(Symbol, Datum)] {
    node(plan, "world/release-plan-v1")
}

#[test]
fn release_plan_copies_service_facts_and_names_each_reason() {
    let service_plan = service::plan_catalog("model").expect("service");
    let service_rows = service_fields(&service_plan);
    let omitted = strings(field(service_rows, "omitted"));
    let retained = vector(service_rows, "retained");
    let probe = omitted
        .first()
        .cloned()
        .or_else(|| {
            retained
                .first()
                .map(|fact| text_at(node(fact, "world/service-fact-v1"), "selector").to_owned())
        })
        .expect("catalog selector");
    let before = service::snapshot("model", &probe).expect("before");
    let plan = release::plan_catalog("model").expect("release");
    assert_eq!(plan, release::plan_catalog("model").expect("repeat"));
    let after = service::snapshot("model", &probe).expect("after");
    assert_eq!(before, after);

    let fields = release_fields(&plan);
    assert!(zero(field(fields, "effects")));
    assert!(zero(field(fields, "subprocesses")));
    assert_eq!(field(fields, "resident"), &Datum::Bool(true));
    assert_eq!(text_at(fields, "disclosure"), DISCLOSURE);
    assert_eq!(text_at(fields, "realization"), "model");
    assert_eq!(
        strings(field(fields, "dependencies")),
        strings(field(service_rows, "ports"))
    );
    let actions = vector(fields, "actions");
    let omissions = vector(fields, "omitted");
    assert_eq!(
        omissions
            .iter()
            .map(|item| text_at(node(item, "world/release-omission-v1"), "selector").to_owned())
            .collect::<Vec<_>>(),
        omitted
    );
    assert_eq!(
        matches!(field(fields, "noop"), Datum::Bool(true)),
        actions.is_empty()
    );

    let mut policies = BTreeSet::new();
    let mut saw_current_omit = false;
    let mut saw_divergent = false;
    let mut saw_noncurrent_equal = false;
    for selector in &omitted {
        let row = snapshot_row("model", selector);
        policies.insert(text_at(&row, "policy").to_owned());
        assert_eq!(text_at(&row, "state"), "current");
        assert!(text_opt(&row, "value").is_some());
        let omission = named(omissions, "world/release-omission-v1", selector).expect("omission");
        assert_eq!(text_at(omission, "reason"), "completed");
        assert_eq!(
            text_at(omission, "intent"),
            format!("intent/reconcile/{selector}")
        );
        assert_eq!(text_at(omission, "disclosure"), DISCLOSURE);
        assert!(named(actions, "world/release-action-v1", selector).is_none());
        saw_current_omit = true;
    }
    assert_eq!(actions.len(), retained.len());
    for fact in retained {
        let kept = node(fact, "world/service-fact-v1");
        let selector = text_at(kept, "selector");
        let row = snapshot_row("model", selector);
        policies.insert(text_at(&row, "policy").to_owned());
        let action = named(actions, "world/release-action-v1", selector).expect("action");
        assert_eq!(text_at(action, "class"), text_at(kept, "class"));
        assert_eq!(text_at(action, "state"), text_at(kept, "state"));
        assert_eq!(text_at(action, "desired"), text_at(kept, "desired"));
        assert_eq!(text_at(action, "port"), text_at(kept, "port"));
        assert_eq!(text_at(action, "provenance"), text_at(kept, "provenance"));
        assert_eq!(text_opt(action, "value"), text_opt(kept, "value"));
        assert_eq!(
            text_at(action, "intent"),
            format!("intent/reconcile/{selector}")
        );
        assert_eq!(text_at(action, "disclosure"), DISCLOSURE);
        let state = text_at(&row, "state");
        if state == "current" {
            assert_eq!(text_at(action, "class"), "divergent");
            assert_eq!(text_at(action, "reason"), "desired-differs");
            assert_ne!(text_at(action, "desired"), text_at(action, "value"));
            saw_divergent = true;
        } else {
            assert_eq!(text_at(action, "class"), "unknown");
            assert_eq!(text_at(action, "reason"), "original-preserved");
            if text_opt(&row, "value") == Some(text_at(action, "desired")) {
                saw_noncurrent_equal = true;
            }
        }
    }
    assert!(saw_current_omit && saw_divergent && saw_noncurrent_equal);
    assert_eq!(
        strings(field(fields, "capabilities")),
        policies.into_iter().collect::<Vec<_>>()
    );
    let rendered = render(&plan);
    assert!(!rendered.contains("sim-private/"));
    assert!(!rendered.contains("bo-private"));
    assert!(!rendered.contains("docs/private"));
    assert!(!rendered.contains("/home/"));
    assert!(!rendered.contains("private-material:"));
    if omitted
        .iter()
        .any(|selector| selector == "mirror/sim-private")
        || retained.iter().any(|fact| {
            text_at(node(fact, "world/service-fact-v1"), "selector") == "mirror/sim-private"
        })
    {
        assert!(rendered.contains("mirror/sim-private"));
    }
}

#[test]
fn release_noop_omits_a_completed_fact_and_preserves_an_uncertain_original() {
    let service_plan = service::plan_catalog("model").expect("service");
    let service_rows = service_fields(&service_plan);
    let mut saw_noop = false;
    for selector in strings(field(service_rows, "omitted")) {
        let row = snapshot_row("model", &selector);
        assert_eq!(text_at(&row, "state"), "current");
        let value = text_opt(&row, "value").expect("current value");
        let plan = release::plan_one("model", &selector, value).expect("noop");
        let fields = release_fields(&plan);
        assert!(vector(fields, "actions").is_empty());
        assert_eq!(field(fields, "noop"), &Datum::Bool(true));
        assert!(zero(field(fields, "effects")));
        assert!(zero(field(fields, "subprocesses")));
        let omission = named(
            vector(fields, "omitted"),
            "world/release-omission-v1",
            &selector,
        )
        .expect("omission");
        assert_eq!(text_at(omission, "reason"), "completed");
        assert_eq!(vector(fields, "omitted").len(), 1);
        saw_noop = true;
    }
    assert!(saw_noop);

    let mut saw_preserved = false;
    let mut saw_withheld = false;
    for fact in vector(service_rows, "retained") {
        let selector = text_at(node(fact, "world/service-fact-v1"), "selector").to_owned();
        let row = snapshot_row("model", &selector);
        let state = text_at(&row, "state").to_owned();
        if state == "current" {
            continue;
        }
        match text_opt(&row, "value").map(str::to_owned) {
            Some(value) => {
                let plan = release::plan_one("model", &selector, &value).expect("preserved");
                let action = named(
                    vector(release_fields(&plan), "actions"),
                    "world/release-action-v1",
                    &selector,
                )
                .expect("action");
                assert_eq!(text_at(action, "class"), "unknown");
                assert_eq!(text_at(action, "reason"), "original-preserved");
                assert_eq!(text_at(action, "state"), state);
                assert_eq!(text_at(action, "value"), value);
                assert_eq!(text_at(action, "desired"), value);
                assert!(zero(field(release_fields(&plan), "effects")));
                saw_preserved = true;
            }
            None => {
                let plan = release::plan_one("model", &selector, "caller-desired-token")
                    .expect("withheld");
                let action = named(
                    vector(release_fields(&plan), "actions"),
                    "world/release-action-v1",
                    &selector,
                )
                .expect("action");
                assert_eq!(text_at(action, "class"), "unknown");
                assert_eq!(text_at(action, "reason"), "original-preserved");
                assert_eq!(text_at(action, "state"), state);
                assert!(text_opt(action, "value").is_none());
                assert_eq!(text_at(action, "desired"), "caller-desired-token");
                saw_withheld = true;
            }
        }
    }
    assert!(saw_preserved && saw_withheld);
}

#[test]
fn release_refuses_private_material_without_echoing_it() {
    let gap = release::plan_one("model", "git/branch/main", "private-material:s3cret-token")
        .expect("gap");
    let fields = node(&gap, "world/release-gap-v1");
    assert_eq!(text_at(fields, "reason"), "private-leakage");
    assert_eq!(text_at(fields, "selector"), "");
    assert!(zero(field(fields, "effects")));
    assert!(zero(field(fields, "subprocesses")));
    let rendered = render(&gap);
    assert!(!rendered.contains("s3cret-token"));
    assert!(!rendered.contains("private-material:"));

    let hidden = release::why("model", "docs/private/note").expect("hidden");
    assert_eq!(
        text_at(node(&hidden, "world/release-gap-v1"), "reason"),
        "private-leakage"
    );
    assert!(!render(&hidden).contains("docs/private"));

    let kept = release::plan_one("model", "mirror/sim-private", "model-commit-main").expect("kept");
    let fields = release_fields(&kept);
    let action = named(
        vector(fields, "actions"),
        "world/release-action-v1",
        "mirror/sim-private",
    )
    .expect("mirror action");
    assert_eq!(text_at(action, "selector"), "mirror/sim-private");
    let rendered = render(&kept);
    assert!(rendered.contains("mirror/sim-private"));
    assert!(!rendered.contains("sim-private/"));
}

#[test]
fn release_native_and_real_keep_unavailable_originals() {
    for realization in ["native", "real"] {
        let service_plan = service::plan_catalog(realization).expect("service");
        let service_rows = service_fields(&service_plan);
        assert!(strings(field(service_rows, "omitted")).is_empty());
        let plan = release::plan_catalog(realization).expect("release");
        let fields = release_fields(&plan);
        assert!(vector(fields, "omitted").is_empty());
        assert!(strings(field(fields, "dependencies")).is_empty());
        assert!(zero(field(fields, "effects")));
        assert!(zero(field(fields, "subprocesses")));
        assert_eq!(field(fields, "noop"), &Datum::Bool(false));
        let retained = vector(service_rows, "retained");
        assert!(!retained.is_empty());
        assert_eq!(vector(fields, "actions").len(), retained.len());
        for fact in retained {
            let kept = node(fact, "world/service-fact-v1");
            let selector = text_at(kept, "selector");
            let row = snapshot_row(realization, selector);
            let action = named(
                vector(fields, "actions"),
                "world/release-action-v1",
                selector,
            )
            .expect("action");
            assert_eq!(text_at(action, "class"), "unknown");
            assert_eq!(text_at(action, "reason"), "original-preserved");
            assert_eq!(text_at(action, "state"), text_at(&row, "state"));
            assert_eq!(text_at(action, "state"), text_at(kept, "state"));
            assert!(text_opt(action, "value").is_none());
            assert!(text_opt(&row, "value").is_none());
            assert!(!text_at(action, "provenance").contains("model/"));
            assert_eq!(text_at(action, "provenance"), text_at(kept, "provenance"));
            assert_eq!(text_at(action, "desired"), text_at(kept, "desired"));
            assert_eq!(
                text_at(action, "intent"),
                format!("intent/reconcile/{selector}")
            );
        }
        assert!(!render(&plan).contains("model/"));
    }
}

#[test]
fn release_status_and_why_read_the_sealed_plan() {
    let plan = release::plan_catalog("model").expect("plan");
    let plan_fields = release_fields(&plan);
    let status = release::status("model").expect("status");
    let status_fields = node(&status, "world/release-status-v1");
    assert_eq!(
        field(status_fields, "omitted"),
        field(plan_fields, "omitted")
    );
    assert_eq!(text_at(status_fields, "disclosure"), DISCLOSURE);
    assert_eq!(field(status_fields, "noop"), field(plan_fields, "noop"));
    assert!(zero(field(status_fields, "effects")));
    assert!(zero(field(status_fields, "subprocesses")));
    let actions = vector(plan_fields, "actions");
    let remaining = vector(status_fields, "remaining");
    assert_eq!(actions.len(), remaining.len());
    for (action, remain) in actions.iter().zip(remaining.iter()) {
        let action = node(action, "world/release-action-v1");
        let remain = node(remain, "world/release-remaining-v1");
        assert_eq!(text_at(remain, "selector"), text_at(action, "selector"));
        assert_eq!(text_at(remain, "reason"), text_at(action, "reason"));
        assert_eq!(text_at(remain, "intent"), text_at(action, "intent"));
    }
    let omissions = vector(plan_fields, "omitted");
    assert!(!omissions.is_empty());
    let done = text_at(node(&omissions[0], "world/release-omission-v1"), "selector").to_owned();
    let why_done = release::why("model", &done).expect("why omitted");
    let why_fields = node(&why_done, "world/release-why-v1");
    assert_eq!(text_at(why_fields, "reason"), "completed");
    assert_eq!(
        text_at(why_fields, "intent"),
        format!("intent/reconcile/{done}")
    );
    assert!(text_opt(why_fields, "value").is_none());
    assert!(text_opt(why_fields, "class").is_none());
    assert!(zero(field(why_fields, "effects")));
    assert!(zero(field(why_fields, "subprocesses")));

    assert!(!actions.is_empty());
    let selector = text_at(node(&actions[0], "world/release-action-v1"), "selector").to_owned();
    let action = node(&actions[0], "world/release-action-v1");
    let why_left = release::why("model", &selector).expect("why remaining");
    let why_fields = node(&why_left, "world/release-why-v1");
    assert_eq!(text_at(why_fields, "reason"), text_at(action, "reason"));
    assert_eq!(text_at(why_fields, "state"), text_at(action, "state"));
    assert_eq!(text_at(why_fields, "class"), text_at(action, "class"));
    assert_eq!(text_opt(why_fields, "value"), text_opt(action, "value"));
    assert!(zero(field(why_fields, "effects")));

    assert!(named(actions, "world/release-action-v1", "git/branch/missing").is_none());
    assert!(named(omissions, "world/release-omission-v1", "git/branch/missing").is_none());
    let absent = release::why("model", "git/branch/missing").expect("absent");
    assert_eq!(
        text_at(node(&absent, "world/release-gap-v1"), "reason"),
        "selector-absent"
    );
    let empty = release::why("model", "").expect("empty");
    assert_eq!(
        text_at(node(&empty, "world/release-gap-v1"), "reason"),
        "empty-selector"
    );
    let host_status = release::status("host").expect("host status");
    assert_eq!(
        text_at(node(&host_status, "world/release-gap-v1"), "reason"),
        "unknown-realization"
    );
}

#[test]
fn release_gaps_reject_unknown_realizations_and_duplicate_selectors() {
    let row = snapshot_row("model", "git/branch/main");
    let token = text_opt(&row, "value").expect("model value").to_owned();
    let host = release::plan_catalog("host").expect("host");
    let rendered = render(&host);
    assert!(!rendered.contains(&token));
    assert!(!rendered.contains("model/"));
    let fields = node(&host, "world/release-gap-v1");
    assert_eq!(text_at(fields, "reason"), "unknown-realization");
    assert_eq!(text_at(fields, "realization"), "host");
    assert_eq!(text_at(fields, "selector"), "");
    assert!(zero(field(fields, "effects")));
    assert!(zero(field(fields, "subprocesses")));
    assert_eq!(
        release::why("host", "git/branch/main").expect("why host"),
        host
    );

    let leaked_name = release::plan_catalog("private-material:s3cret-token").expect("name");
    assert!(!render(&leaked_name).contains("s3cret-token"));
    assert_eq!(
        text_at(node(&leaked_name, "world/release-gap-v1"), "reason"),
        "private-leakage"
    );
    assert_eq!(
        text_at(node(&leaked_name, "world/release-gap-v1"), "realization"),
        ""
    );
    let hidden_home = release::why("/home/secret-path", "git/branch/main").expect("home");
    assert!(!render(&hidden_home).contains("/home/"));

    let empty = release::plan_one("model", "", "value").expect("empty");
    assert_eq!(
        text_at(node(&empty, "world/release-gap-v1"), "reason"),
        "empty-selector"
    );

    let error = release::plan(
        "model",
        &[("git/branch/main", "one"), ("git/branch/main", "two")],
    )
    .expect_err("duplicate");
    match error {
        WorldError::Qualification(message) => {
            assert!(message.contains("duplicate selector"));
        }
        other => panic!("qualification, got {other}"),
    }
}
