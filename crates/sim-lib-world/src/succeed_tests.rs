// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::observe;
use crate::seal;
use crate::succeed;

fn export() -> String {
    "\
export world/succession-export-v1
package sim-incremental-core 0.5.2 registry
package sim-storage-port 0.2.2 registry
package sim-lib-world 0.1.1 registry
project lantern
input provider/widget observed widget-body
input remote/hostile unavailable -
"
    .to_owned()
}

fn change() -> &'static str {
    "shade-1 lantern provider/shade observed shade-next"
}

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

fn semantic_of(snapshot: &Datum) -> String {
    let rows = fields(snapshot, "world/snapshot-v1");
    let Datum::Vector(observations) = field(rows, "observations") else {
        panic!("observations");
    };
    text(
        fields(&observations[0], "world/observation-v1"),
        "semantic-id",
    )
}

fn gap_reason(value: &Datum) -> String {
    text(fields(value, "world/succession-gap-v1"), "reason")
}

#[test]
fn fresh_change_closes_and_the_repeat_does_no_work() {
    let _ = observe::take_declared_calls();
    let record = succeed::run(&export(), change()).expect("succession");
    let calls = observe::take_declared_calls();
    let rows = fields(&record, "world/succession-v1");
    let direct = observe::declared_snapshot(
        "export",
        "provider",
        "provider/widget",
        "observed",
        Some("widget-body"),
    )
    .expect("direct");
    let change_direct = observe::declared_snapshot(
        "change",
        "provider",
        "provider/shade",
        "observed",
        Some("shade-next"),
    )
    .expect("change");
    let fixture = observe::snapshot("lantern", "provider/widget").expect("fixture");
    let Datum::Vector(input_ids) = field(rows, "input-ids") else {
        panic!("input ids");
    };
    let Datum::String(first_id) = &input_ids[0] else {
        panic!("first id");
    };
    assert_eq!(first_id, &semantic_of(&direct));
    assert_eq!(text(rows, "change-id"), semantic_of(&change_direct));
    assert_ne!(text(rows, "change-id"), semantic_of(&fixture));
    assert_eq!(text(rows, "change"), "shade-1");
    assert_eq!(text(rows, "project"), "lantern");
    assert_eq!(text(rows, "origin"), "registry");
    assert_eq!(text(rows, "closed"), "portable");
    assert_eq!(text(rows, "linux"), "absent");
    assert_eq!(text(rows, "recovery"), "match");
    assert_eq!(count(rows, "observed"), 3);
    assert_eq!(count(rows, "proved"), 3);
    assert_eq!(
        calls,
        count(rows, "observed") + count(rows, "proved") + count(rows, "recovery-reads")
    );
    assert_eq!(count(rows, "seal"), 2);
    assert_eq!(count(rows, "converge-first"), 1);
    assert_eq!(count(rows, "converge-second"), 0);
    assert_eq!(count(rows, "land"), 1);
    assert_eq!(count(rows, "deliver-first"), 1);
    assert_eq!(count(rows, "deliver-second"), 0);
    assert_eq!(count(rows, "recovery-reads"), 2);
    assert_eq!(count(rows, "holds"), 0);
    assert_eq!(count(rows, "effects"), 0);
    assert_eq!(count(rows, "subprocesses"), 0);
    assert_ne!(text(rows, "preimage"), text(rows, "postimage"));
    assert_eq!(text(rows, "preimage").len(), 64);
    let body = text(rows, "record");
    assert!(!body.contains('\n'));
    assert!(body.contains("closed portable"));
    assert!(body.contains("converge-second 0"));
    assert!(body.contains("deliver-second 0"));
    assert!(body.contains("change shade-1"));
    assert!(!body.contains("change lantern"));
    let again = succeed::run(&export(), change()).expect("again");
    let again_rows = fields(&again, "world/succession-v1");
    assert_eq!(text(again_rows, "postimage"), text(rows, "postimage"));
    assert_eq!(text(again_rows, "record"), body);
}

#[test]
fn a_different_change_value_seals_a_different_postimage() {
    let first = succeed::run(&export(), change()).expect("first");
    let second = succeed::run(
        &export(),
        "shade-1 lantern provider/shade observed shade-other",
    )
    .expect("second");
    let left = fields(&first, "world/succession-v1");
    let right = fields(&second, "world/succession-v1");
    assert_ne!(text(left, "change-id"), text(right, "change-id"));
    assert_ne!(text(left, "postimage"), text(right, "postimage"));
    assert_eq!(text(left, "preimage"), text(right, "preimage"));
}

#[test]
fn construction_fixture_names_stay_open() {
    let mut names = vec![
        "lantern",
        "lantern-alias",
        "lantern-revised",
        "lantern-stub",
        "model",
        "revised",
        "envelope",
        "native",
        "real",
    ];
    names.extend(seal::local_batches().iter().copied());
    for name in names {
        let record = succeed::run(
            &export(),
            &format!("{name} lantern provider/shade observed shade-next"),
        )
        .expect("gap");
        assert_eq!(gap_reason(&record), "fixture-change");
        assert_eq!(
            text(fields(&record, "world/succession-gap-v1"), "closed"),
            "open"
        );
    }
}

#[test]
fn a_warm_change_token_stays_open() {
    for change_id in [
        "checkout",
        "my-transcript",
        "foo.git",
        "a..b",
        "Checkout",
        "file://x",
        "/tmp",
        "cache/obj",
    ] {
        let record = succeed::run(
            &export(),
            &format!("{change_id} lantern provider/shade observed shade-next"),
        )
        .expect("warm change");
        assert_eq!(gap_reason(&record), "warm-state", "{change_id}");
    }
    let malformed = succeed::run(
        &export(),
        "bad:token lantern provider/shade observed shade-next",
    )
    .expect("malformed");
    assert_eq!(gap_reason(&malformed), "bad-record");
}

#[test]
fn warm_path_private_and_linux_claims_stay_open() {
    let mut warm_export = export();
    warm_export.push_str("input provider/path observed /tmp/widget\n");
    assert_eq!(
        gap_reason(&succeed::run(&warm_export, change()).expect("warm")),
        "warm-state"
    );
    let private_export = export().replace("widget-body", "bo-private");
    assert_eq!(
        gap_reason(&succeed::run(&private_export, change()).expect("private")),
        "private-material"
    );
    let mut claimed = export();
    claimed.push_str("claim reboot\n");
    assert_eq!(
        gap_reason(&succeed::run(&claimed, change()).expect("linux")),
        "linux-claim"
    );
}

#[test]
fn path_origin_stays_open_and_another_public_project_closes() {
    let path_export = export().replace("0.1.1 registry", "0.1.1 path");
    assert_eq!(
        gap_reason(&succeed::run(&path_export, change()).expect("path")),
        "path-origin"
    );
    let other = export().replace("project lantern", "project harbor");
    let record =
        succeed::run(&other, "shade-1 harbor provider/shade observed shade-next").expect("project");
    let rows = fields(&record, "world/succession-v1");
    assert_eq!(text(rows, "project"), "harbor");
    assert_eq!(text(rows, "closed"), "portable");
    let mismatched = succeed::run(
        &export(),
        "shade-1 harbor provider/shade observed shade-next",
    )
    .expect("mismatch");
    assert_eq!(gap_reason(&mismatched), "project-mismatch");
    let mut branched = export();
    branched.push_str("branch private\n");
    assert_eq!(
        gap_reason(&succeed::run(&branched, change()).expect("branch")),
        "private-branch"
    );
    let source = include_str!("succeed.rs");
    assert!(!source.contains("FOREIGN_PROJECT"));
    assert!(!source.contains("\"lantern\""));
}

#[test]
fn an_uncertain_input_is_not_closed() {
    let mut held = export();
    held.push_str("input ci/run uncertain -\n");
    let record = succeed::run(&held, change()).expect("held");
    let rows = fields(&record, "world/succession-v1");
    assert_eq!(text(rows, "closed"), "open");
    assert!(count(rows, "holds") >= 1);
    assert_eq!(count(rows, "converge-first"), 0);
    assert_eq!(count(rows, "converge-second"), 0);
    assert_eq!(count(rows, "deliver-first"), 0);
    assert_eq!(count(rows, "deliver-second"), 0);
    assert_eq!(text(rows, "linux"), "absent");
    assert_eq!(text(rows, "recovery"), "held");
}

#[test]
fn an_effect_or_subprocess_line_is_counted_and_stays_open() {
    let mut effect = export();
    effect.push_str("effect widget\n");
    let record = succeed::run(&effect, change()).expect("effect");
    let rows = fields(&record, "world/succession-gap-v1");
    assert_eq!(text(rows, "reason"), "host-effect");
    assert_eq!(count(rows, "effects"), 1);
    assert_eq!(count(rows, "subprocesses"), 0);
    let mut process = export();
    process.push_str("subprocess worker\n");
    let record = succeed::run(&process, change()).expect("process");
    let rows = fields(&record, "world/succession-gap-v1");
    assert_eq!(text(rows, "reason"), "host-effect");
    assert_eq!(count(rows, "effects"), 0);
    assert_eq!(count(rows, "subprocesses"), 1);
}

#[test]
fn a_fixture_row_under_a_fresh_name_stays_open() {
    let mut cases = vec![
        (
            export(),
            "shade-1 lantern provider/generated observed generated-widget",
        ),
        (
            export(),
            "shade-1 lantern provider/widget observed widget-next",
        ),
    ];
    let bare = "\
export world/succession-export-v1
package sim-lib-world 0.1.1 registry
project lantern
input provider/shade observed shade-kept
";
    cases.push((
        bare.to_owned(),
        "shade-1 lantern remote/hostile unavailable -",
    ));
    for (export_text, change_text) in cases {
        let record = succeed::run(&export_text, change_text).expect("fixture row");
        assert_eq!(gap_reason(&record), "fixture-change", "{change_text}");
        let rows = fields(&record, "world/succession-gap-v1");
        assert_eq!(text(rows, "closed"), "open");
        assert_eq!(count(rows, "effects"), 0);
        assert_eq!(count(rows, "subprocesses"), 0);
    }
}

#[test]
fn a_builtin_row_under_a_fresh_name_stays_open() {
    for change_text in [
        "shade-1 lantern filesystem/readme observed verified-source",
        "shade-1 lantern filesystem/secret denied -",
        "shade-1 lantern git/HEAD observed abc",
        "shade-1 lantern process/probe denied -",
    ] {
        let record = succeed::run(&export(), change_text).expect("builtin row");
        assert_eq!(gap_reason(&record), "fixture-change", "{change_text}");
        assert_eq!(
            text(fields(&record, "world/succession-gap-v1"), "closed"),
            "open"
        );
    }
}

#[test]
fn a_service_row_under_a_fresh_name_stays_open() {
    for change_text in [
        "shade-2 lantern mirror/sim-private denied -",
        "shade-2 lantern ci/sim-sdk uncertain -",
    ] {
        let _ = observe::take_declared_calls();
        let record = succeed::run(&export(), change_text).expect("service row");
        assert_eq!(observe::take_declared_calls(), 3, "{change_text}");
        assert_eq!(gap_reason(&record), "fixture-change", "{change_text}");
        assert_eq!(
            text(fields(&record, "world/succession-gap-v1"), "closed"),
            "open"
        );
    }
}

#[test]
fn repeating_an_exported_value_is_not_a_change() {
    let record = succeed::run(
        &export(),
        "shade-1 lantern provider/widget observed widget-body",
    )
    .expect("empty");
    assert_eq!(gap_reason(&record), "empty-change");
}
