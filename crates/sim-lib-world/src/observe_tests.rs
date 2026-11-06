// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::observe;

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

fn field_opt<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

fn vector(value: &Datum) -> &[Datum] {
    match value {
        Datum::Vector(items) => items,
        other => panic!("vector, got {other:?}"),
    }
}

fn text(value: &Datum) -> &str {
    match value {
        Datum::String(value) => value,
        other => panic!("string, got {other:?}"),
    }
}

fn strings(value: &Datum) -> Vec<String> {
    vector(value)
        .iter()
        .map(|item| text(item).to_owned())
        .collect()
}

fn observation(batch: &str, selector: &str) -> (Datum, Vec<(Symbol, Datum)>) {
    let value = observe::snapshot(batch, selector).expect("snapshot");
    let fields = node(&value, "world/snapshot-v1").to_vec();
    let row = {
        let observations = vector(field(&fields, "observations"));
        assert_eq!(observations.len(), 1);
        node(&observations[0], "world/observation-v1").to_vec()
    };
    (value, row)
}

#[test]
fn snapshot_statuses_stay_distinct_and_reads_match_the_selector() {
    let cases = [
        ("filesystem/readme", "observed", "filesystem", true),
        ("filesystem/secret", "denied", "filesystem", false),
        ("process/probe", "unavailable", "process", false),
        ("git/HEAD", "timed-out", "git", false),
        ("registry/index", "absent", "registry", false),
        ("ci/run", "uncertain", "ci", false),
    ];
    let mut seen = BTreeSet::new();
    for (selector, status, port, has_value) in cases {
        let (value, row) = observation("model", selector);
        let again = observe::snapshot("model", selector).expect("repeat");
        assert_eq!(value, again);
        let fields = node(&value, "world/snapshot-v1");
        assert_eq!(strings(field(fields, "reads")), vec![selector.to_owned()]);
        assert_eq!(strings(field(fields, "ports")), vec![port.to_owned()]);
        assert_eq!(text(field(&row, "status")), status);
        assert_eq!(text(field(&row, "policy")), "policy/explicit-model-v1");
        assert!(!text(field(&row, "freshness")).is_empty());
        assert!(!text(field(&row, "provenance")).is_empty());
        assert_eq!(field_opt(&row, "value").is_some(), has_value);
        let rendered = render(&value);
        assert!(!rendered.contains("filesystem/secret") || selector == "filesystem/secret");
        assert!(!rendered.contains("git/HEAD") || selector == "git/HEAD");
        seen.insert(status);
    }
    assert_eq!(
        seen,
        BTreeSet::from([
            "observed",
            "denied",
            "unavailable",
            "timed-out",
            "absent",
            "uncertain",
        ])
    );
}

#[test]
fn unchanged_batches_do_not_read_a_port() {
    let same = observe::diff("model", "model", "filesystem/readme").expect("same");
    let fields = node(&same, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(strings(field(fields, "reads")).is_empty());
    assert!(strings(field(fields, "ports")).is_empty());
    assert!(field_opt(fields, "before-status").is_none());

    let held = observe::diff("model", "revised", "filesystem/readme").expect("held");
    let fields = node(&held, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(strings(field(fields, "reads")).is_empty());

    let quiet = observe::diff("model", "envelope", "git/HEAD").expect("envelope");
    let fields = node(&quiet, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(strings(field(fields, "reads")).is_empty());
}

#[test]
fn changed_diff_keeps_statuses_and_limits_the_port() {
    let changed = observe::diff("model", "revised", "git/HEAD").expect("git");
    let fields = node(&changed, "world/observation-diff-v1");
    assert_eq!(text(field(fields, "before-status")), "timed-out");
    assert_eq!(text(field(fields, "after-status")), "observed");
    assert!(field_opt(fields, "before-value").is_none());
    assert_eq!(text(field(fields, "after-value")), "abc");
    assert_eq!(
        strings(field(fields, "reads")),
        vec!["git/HEAD".to_owned(), "git/HEAD".to_owned()]
    );
    assert_eq!(
        strings(field(fields, "ports")),
        vec!["git".to_owned(), "git".to_owned()]
    );

    let denied = observe::diff("model", "revised", "process/probe").expect("probe");
    let fields = node(&denied, "world/observation-diff-v1");
    assert_eq!(text(field(fields, "before-status")), "unavailable");
    assert_eq!(text(field(fields, "after-status")), "denied");
    assert_eq!(
        strings(field(fields, "ports")),
        vec!["process".to_owned(), "process".to_owned()]
    );
}

#[test]
fn envelopes_stay_out_of_semantic_identity() {
    let (_, model) = observation("model", "filesystem/readme");
    let (_, envelope) = observation("envelope", "filesystem/readme");
    assert_eq!(
        text(field(&model, "semantic-id")),
        text(field(&envelope, "semantic-id"))
    );
    assert_eq!(
        text(field(&model, "freshness")),
        text(field(&envelope, "freshness"))
    );
    assert_ne!(
        text(field(&model, "envelope")),
        text(field(&envelope, "envelope"))
    );
    assert_eq!(text(field(&model, "value")), "verified-source");
    assert_eq!(
        text(field(&model, "provenance")),
        "verified-source/filesystem/readme"
    );
}

#[test]
fn native_realization_does_not_fall_back() {
    let native = observe::snapshot("native", "filesystem/readme").expect("native");
    let rendered = render(&native);
    assert!(!rendered.contains("verified-source"));
    let fields = node(&native, "world/snapshot-v1");
    assert!(strings(field(fields, "reads")).is_empty());
    assert!(strings(field(fields, "ports")).is_empty());
    let row = node(
        &vector(field(fields, "observations"))[0],
        "world/observation-v1",
    )
    .to_vec();
    assert_eq!(text(field(&row, "status")), "unavailable");
    assert_eq!(text(field(&row, "provenance")), "native-capability-missing");
    assert_eq!(field(&row, "matched"), &Datum::Bool(false));
    assert!(field_opt(&row, "value").is_none());

    let crossed = observe::diff("native", "model", "filesystem/readme").expect("cross");
    let fields = node(&crossed, "world/observation-diff-v1");
    assert_eq!(text(field(fields, "before-status")), "unavailable");
    assert_eq!(text(field(fields, "after-status")), "observed");
    assert!(field_opt(fields, "before-value").is_none());
    assert_eq!(text(field(fields, "after-value")), "verified-source");
    assert_eq!(
        strings(field(fields, "reads")),
        vec!["filesystem/readme".to_owned()]
    );
    assert_eq!(
        strings(field(fields, "ports")),
        vec!["filesystem".to_owned()]
    );

    let (_, model) = observation("model", "process/probe");
    assert_eq!(text(field(&model, "status")), "unavailable");
    assert_eq!(field(&model, "matched"), &Datum::Bool(true));
    assert_ne!(
        text(field(&model, "provenance")),
        text(field(&row, "provenance"))
    );
}

#[test]
fn unknown_inputs_stay_gaps() {
    let unknown = observe::snapshot("nope", "filesystem/readme").expect("batch");
    let fields = node(&unknown, "world/observation-gap-v1");
    assert_eq!(text(field(fields, "reason")), "unknown-batch");
    assert_eq!(field(fields, "effects"), &crate::query::count_datum(0));

    let empty = observe::diff("model", "model", "").expect("empty");
    let fields = node(&empty, "world/observation-gap-v1");
    assert_eq!(text(field(fields, "reason")), "empty-selector");

    let miss = observe::snapshot("model", "filesystem/nope").expect("miss");
    let fields = node(&miss, "world/snapshot-v1");
    assert_eq!(
        strings(field(fields, "reads")),
        vec!["filesystem/nope".to_owned()]
    );
    assert_eq!(
        strings(field(fields, "ports")),
        vec!["filesystem".to_owned()]
    );
    let row = node(
        &vector(field(fields, "observations"))[0],
        "world/observation-v1",
    )
    .to_vec();
    assert_eq!(text(field(&row, "status")), "absent");
    assert_eq!(text(field(&row, "provenance")), "selector-miss");
    assert_eq!(field(&row, "matched"), &Datum::Bool(false));
}
