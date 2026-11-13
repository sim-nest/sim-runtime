// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use crate::WorldError;
use crate::prove_command::ProveCommandLib;

/// One host argument above this size is refused before the process starts.
const HOST_ARGUMENT_LIMIT: usize = 131_072;
const CHUNK: usize = 100_000;

#[test]
fn a_split_union_view_dispatches_like_one_string() {
    let prove = ProveCommandLib::new();
    let cold = prove.run(&argv(&["prove", "run", "cold"])).expect("cold");
    let view = text(&cold, "view");
    let header = header_literal(include_str!("portable_union.rs"));
    assert!(view.starts_with(header));
    assert!(
        view.len() > HOST_ARGUMENT_LIMIT,
        "cold view is {} bytes",
        view.len()
    );
    let parts = chunks(&view);
    assert_eq!(parts.join(""), view);
    assert!(parts.iter().all(|part| part.len() <= CHUNK));
    assert!(parts[0].starts_with(header));

    let whole_status = prove
        .run(&with_view(["prove", "status"], [&view]))
        .expect("status");
    let split_status = prove
        .run(&with_view(["prove", "status"], &parts))
        .expect("split status");
    assert_eq!(split_status, whole_status);
    assert_eq!(
        number(&whole_status, "denominator"),
        number(&cold, "denominator")
    );
    assert_eq!(number(&whole_status, "executed"), number(&cold, "executed"));

    let whole_frontier = prove
        .run(&with_view(["prove", "frontier"], [&view]))
        .expect("frontier");
    let split_frontier = prove
        .run(&with_view(["prove", "frontier"], &parts))
        .expect("split frontier");
    assert_eq!(split_frontier, whole_frontier);
    assert!(bool_field(&whole_frontier, "complete"));
    assert_eq!(
        number(&whole_frontier, "denominator"),
        number(&cold, "denominator")
    );
    assert_eq!(
        number(&whole_frontier, "executed"),
        number(&cold, "executed")
    );

    let whole_leaf = prove
        .run(&with_view(["prove", "frontier", "phase/28"], [&view]))
        .expect("leaf frontier");
    let split_leaf = prove
        .run(&with_view(["prove", "frontier", "phase/28"], &parts))
        .expect("split leaf frontier");
    assert_eq!(split_leaf, whole_leaf);
    assert_eq!(whole_leaf, whole_frontier);

    let whole_run = prove
        .run(&with_view(["prove", "run", "D1"], [&view]))
        .expect("reuse");
    let split_run = prove
        .run(&with_view(["prove", "run", "D1"], &parts))
        .expect("split reuse");
    assert_eq!(split_run, whole_run);
    assert_eq!(number(&whole_run, "reused"), number(&cold, "executed"));

    let whole_why = prove
        .run(&with_view(["prove", "why", "D2", "phase/28"], [&view]))
        .expect("why");
    let split_why = prove
        .run(&with_view(["prove", "why", "D2", "phase/28"], &parts))
        .expect("split why");
    assert_eq!(split_why, whole_why);
    assert!(bool_field(&whole_why, "reused"));

    let token = text(&whole_frontier, "continuation");
    let whole_resume = prove
        .run(&with_view(["prove", "resume", &token], [&view]))
        .expect("resume");
    let split_resume = prove
        .run(&with_view(["prove", "resume", &token], &parts))
        .expect("split resume");
    assert_eq!(split_resume, whole_resume);
    assert_eq!(whole_resume, whole_frontier);
}

#[test]
fn view_headers_are_not_leaves_and_unheaded_tails_are_usage() {
    let prove = ProveCommandLib::new();
    for header in [
        header_literal(include_str!("portable_union.rs")),
        header_literal(include_str!("prove_view.rs")),
    ] {
        let one = prove
            .run(&argv(&["prove", "frontier", header]))
            .expect("header frontier");
        assert_eq!(text(&one, "call"), "frontier");
        assert_eq!(text(&one, "reason"), "refresh-needed");
        let joined = format!("{header} tail");
        let split = prove
            .run(&argv(&["prove", "frontier", header, " tail"]))
            .expect("split header");
        let whole = prove
            .run(&argv(&["prove", "frontier", &joined]))
            .expect("joined header");
        assert_eq!(split, whole);
        assert_eq!(text(&split, "reason"), "refresh-needed");
    }

    let native = prove
        .run(&argv(&["prove", "frontier", "leaf/native-worker-adapter"]))
        .expect("native leaf");
    assert_eq!(text(&native, "reason"), "excluded-scope");
    let assay = prove
        .run(&argv(&["prove", "frontier", "leaf/native-assay"]))
        .expect("assay leaf");
    assert_eq!(text(&assay, "reason"), "excluded-scope");
    let missing = prove
        .run(&argv(&["prove", "frontier", "leaf/not-in-the-union"]))
        .expect("missing leaf");
    assert_eq!(text(&missing, "reason"), "unavailable-proof");
    let named = prove
        .run(&argv(&["prove", "frontier", "phase/28"]))
        .expect("named gate");
    assert_eq!(text(&named, "reason"), "refresh-needed");

    let stale = prove
        .run(&argv(&["prove", "status", "not-a-view"]))
        .expect("stale status");
    assert_eq!(text(&stale, "reason"), "refresh-needed");
    let stale_run = prove
        .run(&argv(&["prove", "run", "D1", "not-a-view"]))
        .expect("stale run");
    assert_eq!(text(&stale_run, "reason"), "refresh-needed");
    let stale_why = prove
        .run(&argv(&["prove", "why", "D2", "phase/28", "not-a-view"]))
        .expect("stale why");
    assert_eq!(text(&stale_why, "reason"), "refresh-needed");
    assert_usage(&["prove", "status", "not-a-view", "tail"]);
    assert_usage(&["prove", "frontier", "leaf/not-in-the-union", "tail"]);
    assert_usage(&["prove", "frontier", "phase/28", "not-a-view"]);
    assert_usage(&["prove", "plan", "token", "tail"]);
    assert_usage(&["prove", "run", "D1", "not-a-view", "tail"]);
    assert_usage(&["prove", "why", "D2", "phase/28", "not-a-view", "tail"]);
    assert_usage(&["prove", "resume", "token", "not-a-view", "tail"]);
    assert_usage(&["prove", "measure", "status", "tail"]);
}

fn chunks(view: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = view;
    while !rest.is_empty() {
        let mut end = CHUNK.min(rest.len());
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        assert!(end > 0);
        parts.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    assert!(parts.len() > 1);
    parts
}

fn header_literal(source: &str) -> &str {
    let marker = "const HEADER: &str = \"";
    let start = source.find(marker).expect("header") + marker.len();
    let end = source[start..].find('"').expect("header end");
    &source[start..start + end]
}

fn assert_usage(args: &[&str]) {
    let err = ProveCommandLib::new().run(&argv(args)).expect_err("usage");
    match err {
        WorldError::Qualification(reason) => assert_eq!(reason, "usage"),
        other => panic!("usage refusal, got {other}"),
    }
}

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().copied().map(str::to_owned).collect()
}

fn with_view(
    head: impl IntoIterator<Item = impl AsRef<str>>,
    parts: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<String> {
    head.into_iter()
        .map(|part| part.as_ref().to_owned())
        .chain(parts.into_iter().map(|part| part.as_ref().to_owned()))
        .collect()
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
