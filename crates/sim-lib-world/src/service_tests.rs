// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::service;

const SELECTORS: &[(&str, &str, &str, &str)] = &[
    ("git/branch/main", "branch", "current", "git"),
    ("git/branch/topic", "branch", "current", "git"),
    ("git/tag/v1", "tag", "current", "git"),
    ("git/direct/v1", "direct", "current", "git"),
    ("git/peeled/v1", "peeled", "current", "git"),
    (
        "registry/checksum/sim-kernel",
        "checksum",
        "current",
        "registry",
    ),
    ("ci/sim-kernel", "ci", "stale", "ci"),
    ("ci/sim-run", "ci", "partial", "ci"),
    ("ci/sim-sdk", "ci", "uncertain", "ci"),
    ("mirror/sim-kernel", "mirror", "lagging", "mirror"),
    ("mirror/sim-private", "mirror", "denied", "mirror"),
];

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

fn strings(value: &Datum) -> Vec<String> {
    match value {
        Datum::Vector(items) => items.iter().map(text).map(str::to_owned).collect(),
        other => panic!("vector, got {other:?}"),
    }
}

fn zero(value: &Datum) -> bool {
    matches!(value, Datum::Number(number) if number.canonical == "0")
}

fn snapshot_row(realization: &str, selector: &str) -> (Datum, Vec<(Symbol, Datum)>) {
    let value = service::snapshot(realization, selector).expect("snapshot");
    let fields = node(&value, "world/service-snapshot-v1").to_vec();
    let row = node(
        &match field(&fields, "observations") {
            Datum::Vector(items) => items[0].clone(),
            other => panic!("observations, got {other:?}"),
        },
        "world/service-observation-v1",
    )
    .to_vec();
    (value, row)
}

fn retained<'a>(plan: &'a Datum, selector: &str) -> Option<&'a [(Symbol, Datum)]> {
    let fields = node(plan, "world/service-plan-v1");
    match field(fields, "retained") {
        Datum::Vector(items) => items.iter().find_map(|item| {
            let row = node(item, "world/service-fact-v1");
            (text(field(row, "selector")) == selector).then_some(row)
        }),
        other => panic!("retained, got {other:?}"),
    }
}

#[test]
fn service_readings_keep_exact_targets_and_distinct_states() {
    let mut states = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for (selector, kind, state, port) in SELECTORS {
        let (value, row) = snapshot_row("model", selector);
        let again = service::snapshot("model", selector).expect("repeat");
        assert_eq!(value, again);
        let fields = node(&value, "world/service-snapshot-v1");
        assert!(zero(field(fields, "effects")));
        assert!(zero(field(fields, "subprocesses")));
        assert_eq!(strings(field(fields, "reads")), vec![selector.to_owned()]);
        assert_eq!(strings(field(fields, "ports")), vec![port.to_owned()]);
        assert_eq!(text(field(&row, "kind")), *kind);
        assert_eq!(text(field(&row, "state")), *state);
        assert_eq!(text(field(&row, "port")), *port);
        assert_eq!(text(field(&row, "provenance")), format!("model/{selector}"));
        assert!(field_opt(&row, "desired").is_none());
        let carries = matches!(*state, "current" | "stale" | "partial" | "lagging");
        assert_eq!(field_opt(&row, "value").is_some(), carries);
        states.insert(*state);
        kinds.insert(*kind);
    }
    assert_eq!(
        states,
        BTreeSet::from([
            "current",
            "stale",
            "partial",
            "denied",
            "lagging",
            "uncertain",
        ])
    );
    assert_eq!(
        kinds,
        BTreeSet::from([
            "branch", "tag", "direct", "peeled", "checksum", "ci", "mirror",
        ])
    );

    let (_, direct) = snapshot_row("model", "git/direct/v1");
    let (_, peeled) = snapshot_row("model", "git/peeled/v1");
    let (_, tag) = snapshot_row("model", "git/tag/v1");
    assert_eq!(text(field(&direct, "value")), text(field(&tag, "value")));
    assert_ne!(text(field(&peeled, "value")), text(field(&direct, "value")));
}

#[test]
fn plan_omits_current_matches_and_retains_the_rest_without_mutation() {
    let before = service::snapshot("model", "git/branch/topic").expect("before");
    let catalog = service::plan_catalog("model").expect("catalog");
    let after = service::snapshot("model", "git/branch/topic").expect("after");
    assert_eq!(before, after);
    assert_eq!(catalog, service::plan_catalog("model").expect("repeat"));
    let fields = node(&catalog, "world/service-plan-v1");
    assert!(zero(field(fields, "effects")));
    assert!(zero(field(fields, "subprocesses")));
    assert_eq!(text(field(fields, "realization")), "model");
    let omitted = strings(field(fields, "omitted"));
    let mut actions = Vec::new();
    match field(fields, "actions") {
        Datum::Vector(items) => {
            for item in items {
                let action = node(item, "world/service-action-v1");
                assert_eq!(text(field(action, "class")), "divergent");
                actions.push(text(field(action, "selector")).to_owned());
            }
        }
        other => panic!("actions, got {other:?}"),
    }
    assert!(omitted.iter().all(|selector| !actions.contains(selector)));
    let mut saw_current_omit = false;
    let mut saw_current_diverge = false;
    let mut saw_noncurrent_equal = false;
    for (selector, _, _, _) in SELECTORS {
        let (_, row) = snapshot_row("model", selector);
        let state = text(field(&row, "state")).to_owned();
        if omitted.iter().any(|item| item == selector) {
            assert_eq!(state, "current");
            assert!(!actions.iter().any(|item| item == selector));
            assert!(retained(&catalog, selector).is_none());
            saw_current_omit = true;
            continue;
        }
        let kept = retained(&catalog, selector).expect("retained");
        if state == "current" {
            assert_eq!(text(field(kept, "class")), "divergent");
            assert_ne!(text(field(kept, "desired")), text(field(kept, "value")));
            assert!(actions.iter().any(|item| item == selector));
            saw_current_diverge = true;
        } else {
            assert_eq!(text(field(kept, "class")), "unknown");
            assert!(!actions.iter().any(|item| item == selector));
            match field_opt(&row, "value").map(text) {
                Some(value) => {
                    assert_eq!(field_opt(kept, "value").map(text), Some(value));
                    if text(field(kept, "desired")) == value {
                        saw_noncurrent_equal = true;
                    }
                }
                None => assert!(field_opt(kept, "value").is_none()),
            }
        }
    }
    assert!(saw_current_omit);
    assert!(saw_current_diverge);
    assert!(saw_noncurrent_equal);

    for (selector, _, _, _) in SELECTORS {
        let (_, row) = snapshot_row("model", selector);
        let state = text(field(&row, "state")).to_owned();
        let Some(value) = field_opt(&row, "value").map(text).map(str::to_owned) else {
            assert_ne!(state, "current");
            let unknown = service::plan_one("model", selector, "caller-desired").expect("none");
            let kept = retained(&unknown, selector).expect("kept");
            assert_eq!(text(field(kept, "class")), "unknown");
            assert!(field_opt(kept, "value").is_none());
            let unknown_fields = node(&unknown, "world/service-plan-v1");
            assert!(strings(field(unknown_fields, "omitted")).is_empty());
            assert!(strings(field(unknown_fields, "actions")).is_empty());
            continue;
        };
        let matched = service::plan_one("model", selector, &value).expect("match");
        let matched_fields = node(&matched, "world/service-plan-v1");
        if state == "current" {
            assert_eq!(
                strings(field(matched_fields, "omitted")),
                vec![selector.to_owned()]
            );
            assert!(strings(field(matched_fields, "actions")).is_empty());
            assert!(retained(&matched, selector).is_none());
        } else {
            assert!(strings(field(matched_fields, "omitted")).is_empty());
            assert!(strings(field(matched_fields, "actions")).is_empty());
            let kept = retained(&matched, selector).expect("unknown");
            assert_eq!(text(field(kept, "class")), "unknown");
            assert_eq!(text(field(kept, "desired")), value);
            assert_eq!(field_opt(kept, "value").map(text), Some(value.as_str()));
        }
    }
}

#[test]
fn peeled_target_does_not_accept_the_direct_object() {
    let (_, direct) = snapshot_row("model", "git/direct/v1");
    let direct_value = text(field(&direct, "value")).to_owned();
    let plan = service::plan_one("model", "git/peeled/v1", &direct_value).expect("plan");
    let fields = node(&plan, "world/service-plan-v1");
    assert!(strings(field(fields, "omitted")).is_empty());
    let kept = retained(&plan, "git/peeled/v1").expect("peeled");
    assert_eq!(text(field(kept, "class")), "divergent");
    assert_eq!(text(field(kept, "desired")), direct_value);
    assert_ne!(text(field(kept, "value")), direct_value);
    let (_, main) = snapshot_row("model", "git/branch/main");
    let flipped = service::plan_one(
        "model",
        "git/branch/main",
        &format!("{}-other", text(field(&main, "value"))),
    )
    .expect("flip");
    let flipped_row = retained(&flipped, "git/branch/main").expect("flipped");
    assert_eq!(text(field(flipped_row, "class")), "divergent");
}

#[test]
fn native_and_real_requests_do_not_choose_model_readings() {
    let (model, model_row) = snapshot_row("model", "git/branch/topic");
    let token = text(field(&model_row, "value")).to_owned();
    assert!(render(&model).contains(&token));
    for realization in ["native", "real"] {
        let (value, row) = snapshot_row(realization, "git/branch/topic");
        let fields = node(&value, "world/service-snapshot-v1");
        assert!(strings(field(fields, "reads")).is_empty());
        assert!(strings(field(fields, "ports")).is_empty());
        assert!(zero(field(fields, "effects")));
        assert_eq!(text(field(&row, "state")), "unavailable");
        assert!(field_opt(&row, "value").is_none());
        assert!(!render(&value).contains(&token));
        assert!(!render(&value).contains("model/"));
        let plan = service::plan_catalog(realization).expect("plan");
        let plan_fields = node(&plan, "world/service-plan-v1");
        assert!(strings(field(plan_fields, "omitted")).is_empty());
        assert!(strings(field(plan_fields, "reads")).is_empty());
        assert!(strings(field(plan_fields, "actions")).is_empty());
        assert!(!render(&plan).contains("model/"));
        match field(plan_fields, "retained") {
            Datum::Vector(items) => {
                for item in items {
                    let kept = node(item, "world/service-fact-v1");
                    assert!(field_opt(kept, "value").is_none());
                    assert_eq!(text(field(kept, "class")), "unknown");
                    assert_eq!(text(field(kept, "state")), "unavailable");
                }
            }
            other => panic!("retained, got {other:?}"),
        }
    }
    let (_, native) = snapshot_row("native", "git/branch/topic");
    let (_, real) = snapshot_row("real", "git/branch/topic");
    assert_eq!(
        text(field(&native, "provenance")),
        "native-capability-missing"
    );
    assert_eq!(text(field(&real, "provenance")), "real-service-unbound");
    assert_ne!(
        text(field(&native, "provenance")),
        text(field(&model_row, "provenance"))
    );
}

#[test]
fn provenance_survives_observation_encoding() {
    let (snapshot, row) = snapshot_row("model", "git/branch/topic");
    let bytes = service::encode_observation(&snapshot).expect("encode");
    let decoded = service::decode_observation(&bytes).expect("decode");
    let decoded_fields = node(&decoded, "world/service-observation-v1");
    assert_eq!(
        text(field(decoded_fields, "provenance")),
        text(field(&row, "provenance"))
    );
    assert_eq!(
        text(field(decoded_fields, "state")),
        text(field(&row, "state"))
    );
    assert_eq!(
        text(field(decoded_fields, "value")),
        text(field(&row, "value"))
    );
    assert_eq!(decoded_fields, row.as_slice());
    let catalog = service::plan_catalog("model").expect("catalog");
    let topic = retained(&catalog, "git/branch/topic").expect("topic");
    let desired = text(field(topic, "desired"));
    assert_ne!(desired, text(field(&row, "value")));
    assert!(
        !bytes
            .windows(desired.len())
            .any(|window| window == desired.as_bytes())
    );
    let mut trailing = bytes.clone();
    trailing.push(1);
    assert!(service::decode_observation(&trailing).is_err());
    assert!(service::decode_observation(&bytes[..bytes.len() - 1]).is_err());
    assert!(service::decode_observation(b"not-an-observation").is_err());
}

#[test]
fn gaps_and_duplicate_requests_stay_closed() {
    let empty = service::snapshot("model", "").expect("empty");
    assert_eq!(
        text(field(node(&empty, "world/service-gap-v1"), "reason")),
        "empty-selector"
    );
    let (_, model_row) = snapshot_row("model", "git/branch/main");
    let token = text(field(&model_row, "value")).to_owned();
    let unknown = service::snapshot("host", "git/branch/main").expect("host");
    assert_eq!(
        text(field(node(&unknown, "world/service-gap-v1"), "reason")),
        "unknown-realization"
    );
    assert!(!render(&unknown).contains(&token));
    let miss = service::snapshot("model", "git/branch/missing").expect("miss");
    let miss_fields = node(&miss, "world/service-snapshot-v1");
    let miss_observation = match field(miss_fields, "observations") {
        Datum::Vector(items) => items[0].clone(),
        other => panic!("observations, got {other:?}"),
    };
    let miss_row = node(&miss_observation, "world/service-observation-v1");
    assert_eq!(text(field(miss_row, "state")), "absent");
    assert_eq!(
        strings(field(miss_fields, "reads")),
        vec!["git/branch/missing".to_owned()]
    );
    let duplicate = service::plan(
        "model",
        &[("git/branch/main", "one"), ("git/branch/main", "two")],
    );
    assert!(duplicate.is_err());
}
