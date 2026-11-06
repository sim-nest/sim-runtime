// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::canary::{self, PurposeGrant};
use crate::command::render;
use crate::service;

fn fields<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
    match value {
        Datum::Node { tag: found, fields } if found.as_qualified_str() == tag => fields,
        other => panic!("expected {tag}, got {other:?}"),
    }
}

fn field<'a>(rows: &'a [(Symbol, Datum)], name: &str) -> &'a Datum {
    rows.iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text(rows: &[(Symbol, Datum)], name: &str) -> String {
    match field(rows, name) {
        Datum::String(value) => value.clone(),
        other => panic!("{name} text, got {other:?}"),
    }
}

fn count(rows: &[(Symbol, Datum)], name: &str) -> u64 {
    match field(rows, name) {
        Datum::Number(value) => value.canonical.parse().expect("count"),
        other => panic!("{name} count, got {other:?}"),
    }
}

fn nested<'a>(rows: &'a [(Symbol, Datum)], name: &str, tag: &str) -> &'a [(Symbol, Datum)] {
    fields(field(rows, name), tag)
}

fn gap_reason(value: &Datum) -> String {
    text(fields(value, "world/canary-gap-v1"), "reason")
}

fn source_value(selector: &str) -> String {
    let snapshot = service::snapshot("model", selector).expect("source");
    let rows = fields(&snapshot, "world/service-snapshot-v1");
    let Datum::Vector(observations) = field(rows, "observations") else {
        panic!("observations");
    };
    text(
        fields(&observations[0], "world/service-observation-v1"),
        "value",
    )
}

#[test]
fn canary_places_one_fresh_branch_and_repeats_without_work() {
    let head = source_value("git/branch/main");
    let before = render(&service::snapshot("model", "git/branch/main").expect("before"));
    let mut grant = PurposeGrant::issue("world/model-canary");
    let record = canary::run_grant(
        "model",
        "git/branch/main",
        "git/branch/canary-1",
        &mut grant,
    )
    .expect("canary");
    let rows = fields(&record, "world/canary-v1");
    let source = nested(rows, "source", "world/canary-source-v1");
    let destination = nested(rows, "destination", "world/canary-branch-v1");
    assert_eq!(text(source, "value"), head);
    assert_eq!(text(source, "state"), "current");
    assert_eq!(text(source, "kind"), "branch");
    assert!(text(source, "provenance").contains("model/"));
    assert_eq!(text(destination, "selector"), "git/branch/canary-1");
    assert_eq!(text(destination, "value"), head);
    assert_eq!(text(destination, "state"), "current");
    assert!(text(destination, "provenance").contains("model/"));
    assert_eq!(text(rows, "class"), "verified");
    assert_eq!(count(rows, "dispatches"), 1);
    assert_eq!(count(rows, "performer-calls"), 1);
    assert_eq!(count(rows, "repeat-dispatches"), 0);
    assert_eq!(count(rows, "repeat-performer-calls"), 0);
    assert_eq!(count(rows, "effects"), 0);
    assert_eq!(count(rows, "subprocesses"), 0);
    assert!(count(rows, "observations") >= 3);
    assert_eq!(text(rows, "grant-state"), "consumed");
    assert_eq!(text(rows, "grant-purpose"), "world/model-canary");
    assert_eq!(field(rows, "recovered"), &Datum::Bool(true));
    let again = canary::run_grant(
        "model",
        "git/branch/main",
        "git/branch/canary-2",
        &mut grant,
    )
    .expect("second grant");
    assert_eq!(gap_reason(&again), "consumed-purpose");
    assert_eq!(
        count(fields(&again, "world/canary-gap-v1"), "dispatches"),
        0
    );
    let after = render(&service::snapshot("model", "git/branch/main").expect("after"));
    assert_eq!(before, after);
    let repeated = canary::run(
        "model",
        "git/branch/main",
        "git/branch/canary-1",
        "world/model-canary",
        "issued",
    )
    .expect("repeat");
    assert_eq!(render(&record), render(&repeated));
}

#[test]
fn wrong_and_public_targets_do_not_dispatch() {
    let wrong = canary::run(
        "model",
        "git/branch/main",
        "git/branch/canary-3",
        "world/deliver",
        "issued",
    )
    .expect("wrong purpose");
    assert_eq!(gap_reason(&wrong), "wrong-purpose");
    assert!(!render(&wrong).contains("model-commit"));
    let spent = canary::run(
        "model",
        "git/branch/main",
        "git/branch/canary-3",
        "world/model-canary",
        "spent",
    )
    .expect("spent");
    assert_eq!(gap_reason(&spent), "consumed-purpose");
    for destination in [
        "git/branch/main",
        "git/branch/topic",
        "git/tag/v1",
        "registry/checksum/sim-kernel",
        "git/merge/topic",
        "ci/sim-kernel",
        "mirror/sim-kernel",
    ] {
        let refused = canary::run(
            "model",
            "git/branch/main",
            destination,
            "world/model-canary",
            "issued",
        )
        .expect("public");
        assert_eq!(gap_reason(&refused), "public-target", "{destination}");
        assert_eq!(
            count(fields(&refused, "world/canary-gap-v1"), "dispatches"),
            0
        );
        assert!(!render(&refused).contains("model/"));
    }
    for destination in ["git/notes/v1", "ci/missing-job", "mirror/missing-name"] {
        let refused = canary::run(
            "model",
            "git/branch/main",
            destination,
            "world/model-canary",
            "issued",
        )
        .expect("not a branch");
        assert_eq!(
            gap_reason(&refused),
            "destination-not-branch",
            "{destination}"
        );
        assert_eq!(
            count(fields(&refused, "world/canary-gap-v1"), "dispatches"),
            0
        );
        assert!(!render(&refused).contains("model/"));
    }
    let stale = canary::run(
        "model",
        "ci/sim-kernel",
        "git/branch/canary-4",
        "world/model-canary",
        "issued",
    )
    .expect("stale");
    assert_eq!(gap_reason(&stale), "source-not-validated");
    let tag = canary::run(
        "model",
        "git/tag/v1",
        "git/branch/canary-4",
        "world/model-canary",
        "issued",
    )
    .expect("tag source");
    assert_eq!(gap_reason(&tag), "source-not-validated");
    let mirror = canary::run(
        "model",
        "mirror/sim-private",
        "git/branch/canary-4",
        "world/model-canary",
        "issued",
    )
    .expect("mirror");
    assert_eq!(gap_reason(&mirror), "source-not-validated");
    assert!(render(&mirror).contains("mirror/sim-private"));
    assert!(!render(&mirror).contains("sim-private/"));
}

#[test]
fn private_and_unavailable_canaries_redact() {
    let secret = canary::run(
        "model",
        "git/branch/main",
        "git/branch/canary-5",
        "private-material:s3cret-token",
        "issued",
    )
    .expect("private purpose");
    let rendered = render(&secret);
    assert_eq!(gap_reason(&secret), "private-leakage");
    assert!(!rendered.contains("s3cret-token"));
    assert!(!rendered.contains("private-material:"));
    let hidden = canary::run(
        "private-material:s3cret-token",
        "git/branch/main",
        "/home/secret-path",
        "world/model-canary",
        "issued",
    )
    .expect("private destination");
    let hidden_render = render(&hidden);
    assert!(!hidden_render.contains("s3cret-token"));
    assert!(!hidden_render.contains("/home/"));
    assert!(!hidden_render.contains("private-material:"));
    for realization in ["native", "real"] {
        let refused = canary::run(
            realization,
            "git/branch/main",
            "git/branch/canary-5",
            "world/model-canary",
            "issued",
        )
        .expect(realization);
        assert_eq!(gap_reason(&refused), "unavailable-receiver");
        let text = render(&refused);
        assert!(!text.contains("model/"));
        assert!(!text.contains("model-commit"));
        assert!(
            !fields(&refused, "world/canary-gap-v1")
                .iter()
                .any(|(symbol, _)| symbol.as_qualified_str() == "value")
        );
    }
}

#[test]
fn private_observed_head_refuses_before_delivery() {
    let mut grant = PurposeGrant::issue("world/model-canary");
    let secret = "private-material:s3cret-token";
    let refused = canary::deliver_observed_head(
        "model",
        "git/branch/main",
        "git/branch/canary-9",
        &mut grant,
        secret,
    )
    .expect("private head");
    let rendered = render(&refused);
    assert_eq!(gap_reason(&refused), "private-leakage");
    assert_eq!(
        count(fields(&refused, "world/canary-gap-v1"), "dispatches"),
        0
    );
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("private-material:"));
    assert!(!rendered.contains("model/"));
    let head = source_value("git/branch/main");
    let clean = canary::deliver_observed_head(
        "model",
        "git/branch/main",
        "git/branch/canary-9",
        &mut grant,
        &head,
    )
    .expect("clean head");
    let rows = fields(&clean, "world/canary-v1");
    assert_eq!(text(rows, "class"), "verified");
    assert_eq!(count(rows, "dispatches"), 1);
    assert_eq!(
        text(nested(rows, "source", "world/canary-source-v1"), "value"),
        head
    );
    assert!(!render(&clean).contains(secret));
}
