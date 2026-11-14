// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use crate::WorldError;
use crate::command::{WorldCommandLib, render};

const PRIVATE: &str = "private-material:s3cret-token";

#[test]
fn the_loaded_transfer_command_admits_portable_moves() {
    let command = WorldCommandLib::new().expect("command");
    let document = document(&[
        &rows(&[
            ("responsibility", "r3-prove"),
            ("previous", "cli/main/old-prove"),
            ("evidence", "caller-zero/cli/main/old-prove"),
        ]),
        &rows(&[
            ("responsibility", "r4-converge"),
            ("entrypoint", "cli/main/converge"),
            ("previous", "cargo"),
            ("disposition", "retain"),
            ("callers", "4"),
            ("evidence", "none"),
            ("role", "cargo"),
            ("grant", "cap/converge"),
            ("provider", "model/converge"),
        ]),
        &rows(&[
            ("responsibility", "r9-succeed"),
            ("entrypoint", "cli/main/succeed"),
            ("previous", "none"),
            ("disposition", "open"),
            ("evidence", "none"),
            ("grant", "cap/succeed"),
            ("provider", "model/succeed"),
        ]),
        &rows(&[
            ("responsibility", "r1-explain"),
            ("entrypoint", "cli/main/publication-note"),
            ("previous", "cli/main/sudoers"),
            ("evidence", "caller-zero/cli/main/sudoers"),
            ("provider", "model/sudoers"),
        ]),
    ]);
    let first = command.run(&argv(&document)).expect("transfer");
    let second = command.run(&argv(&document)).expect("transfer again");
    assert_eq!(first, second);
    assert_eq!(tag(&first), "world/transfer-v1");
    assert_eq!(count(&first, "transfers"), "4");
    assert_eq!(count(&first, "retired"), "2");
    assert_eq!(count(&first, "retained"), "1");
    assert_eq!(count(&first, "opened"), "1");
    assert_eq!(count(&first, "effects"), "0");
    assert_eq!(count(&first, "subprocesses"), "0");
    assert_eq!(
        text(&first, "responsibilities"),
        "r3-prove,r4-converge,r9-succeed,r1-explain"
    );
    assert_eq!(text(&first, "roles"), "cargo");
    assert_eq!(
        text(&first, "evidence"),
        "caller-zero/cli/main/old-prove,caller-zero/cli/main/sudoers"
    );
    assert_eq!(text(&first, "scope"), "portable");
    assert_eq!(text(&first, "seal"), "unclaimed");
    assert!(flag(&first, "resident"));
    let rendered = render(&first);
    assert!(!rendered.contains("PortableClosed"));
    assert!(!rendered.contains("derive_completion"));
    let source = include_str!("transfer.rs");
    assert!(!source.contains("derive_completion"));
    assert!(!source.contains("PortableClosed"));
}

#[test]
fn each_ordinary_tool_keeps_its_named_role() {
    let command = WorldCommandLib::new().expect("command");
    let document = document(&[
        &retain("r0-witness", "cargo", "2"),
        &retain("r5-land", "git", "0"),
        &retain("r6-reconcile", "validation", "9"),
        &retain("r8-conduct", "operator-recovery", "1"),
    ]);
    let value = command.run(&argv(&document)).expect("roles");
    let again = command.run(&argv(&document)).expect("roles again");
    assert_eq!(value, again);
    assert_eq!(count(&value, "transfers"), "4");
    assert_eq!(count(&value, "retained"), "4");
    assert_eq!(count(&value, "retired"), "0");
    assert_eq!(count(&value, "opened"), "0");
    assert_eq!(count(&value, "effects"), "0");
    assert_eq!(
        text(&value, "roles"),
        "cargo,git,validation,operator-recovery"
    );
    assert_eq!(text(&value, "evidence"), "");
    assert_eq!(text(&value, "seal"), "unclaimed");
}

#[test]
fn retirement_requires_caller_zero_and_refuses_ordinary_tools() {
    let command = WorldCommandLib::new().expect("command");
    let cases = [
        (rows(&[("evidence", "none")]), "r3-prove", "caller-zero"),
        (
            rows(&[("evidence", "caller-zero/cli/main/other")]),
            "r3-prove",
            "caller-zero",
        ),
        (rows(&[("callers", "2")]), "r3-prove", "callers"),
        (
            rows(&[
                ("previous", "cargo"),
                ("evidence", "caller-zero/cargo"),
                ("callers", "0"),
            ]),
            "r3-prove",
            "ordinary-tool",
        ),
        (
            rows(&[
                ("previous", "validation"),
                ("evidence", "caller-zero/validation"),
            ]),
            "r3-prove",
            "ordinary-tool",
        ),
        (
            rows(&[("role", "git"), ("previous", "cli/main/old-prove")]),
            "r3-prove",
            "ordinary-tool",
        ),
        (
            rows(&[
                ("disposition", "retain"),
                ("role", "cargo"),
                ("previous", "git"),
                ("evidence", "none"),
            ]),
            "r3-prove",
            "previous",
        ),
        (
            rows(&[
                ("disposition", "open"),
                ("previous", "none"),
                ("evidence", "none"),
                ("callers", "3"),
            ]),
            "r3-prove",
            "callers",
        ),
    ];
    for (body, responsibility, reason) in cases {
        let document = document(&[&body]);
        let value = command.run(&argv(&document)).expect("gap");
        let again = command.run(&argv(&document)).expect("gap again");
        assert_eq!(value, again);
        assert_gap(&value, responsibility, reason);
    }
}

#[test]
fn native_requests_and_credentials_refuse_before_effects() {
    let command = WorldCommandLib::new().expect("command");
    let native_body = format!("{}\nrequest native", rows(&[("callers", "3")]));
    let cases = [
        (native_body, "r3-prove", "native-request"),
        (
            rows(&[
                ("scope", "linux"),
                ("callers", "3"),
                ("credential", "token"),
            ]),
            "r3-prove",
            "native-request",
        ),
        (rows(&[("scope", "native")]), "r3-prove", "native-request"),
        (rows(&[("scope", "host")]), "r3-prove", "scope"),
        (
            rows(&[("credential", "publish-token")]),
            "r3-prove",
            "model-credential",
        ),
        (
            rows(&[("grant", "cap/publication")]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[("provider", "model/linux/site")]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[("entrypoint", "cli/main/sudo")]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[("entrypoint", "cli/main/native")]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[
                ("previous", "cli/main/publication"),
                ("evidence", "caller-zero/cli/main/publication"),
            ]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[
                ("previous", "cli/main/native"),
                ("evidence", "caller-zero/cli/main/native"),
            ]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[
                ("previous", "cli/main/linux"),
                ("evidence", "caller-zero/cli/main/linux"),
            ]),
            "r3-prove",
            "native-authority",
        ),
        (
            rows(&[
                ("previous", "cli/main/sudo"),
                ("evidence", "caller-zero/cli/main/sudo"),
            ]),
            "r3-prove",
            "native-authority",
        ),
        (rows(&[("provider", "local/site")]), "r3-prove", "provider"),
        (
            rows(&[("responsibility", "r3-publish")]),
            "r3-publish",
            "responsibility",
        ),
    ];
    for (body, responsibility, reason) in cases {
        let value = command.run(&argv(&document(&[&body]))).expect("gap");
        assert_gap(&value, responsibility, reason);
        assert!(!render(&value).contains("PortableClosed"));
    }
    let only_native = command
        .run(&argv("request native"))
        .expect("request native");
    assert_gap(&only_native, "", "native-request");
    let native_after_fault = document(&[
        &rows(&[("evidence", "none")]),
        &rows(&[
            ("responsibility", "r5-land"),
            ("scope", "native"),
            ("previous", "cli/main/old-land"),
            ("evidence", "caller-zero/cli/main/old-land"),
        ]),
    ]);
    assert_gap(
        &command
            .run(&argv(&native_after_fault))
            .expect("native after fault"),
        "r5-land",
        "native-request",
    );
    let linux_after_fault = document(&[
        &rows(&[("evidence", "none")]),
        &rows(&[("responsibility", "r2-observe"), ("scope", "linux")]),
    ]);
    assert_gap(
        &command
            .run(&argv(&linux_after_fault))
            .expect("linux after fault"),
        "r2-observe",
        "native-request",
    );
    let native_private = format!("request native\n{PRIVATE}");
    let native_private_gap = command
        .run(&argv(&native_private))
        .expect("native before leakage");
    assert_gap(&native_private_gap, "", "native-request");
    let native_private_text = render(&native_private_gap);
    assert!(!native_private_text.contains(PRIVATE));
    assert!(!native_private_text.contains("s3cret-token"));
    let private_then_native = document(&[
        &format!("{}\n{PRIVATE}", rows(&[("evidence", "none")])),
        &rows(&[
            ("responsibility", "r5-land"),
            ("scope", "native"),
            ("previous", "cli/main/old-land"),
            ("evidence", "caller-zero/cli/main/old-land"),
        ]),
    ]);
    let hidden_native = command
        .run(&argv(&private_then_native))
        .expect("scope after leakage");
    assert_gap(&hidden_native, "r5-land", "native-request");
    assert!(!render(&hidden_native).contains(PRIVATE));
    assert!(!render(&hidden_native).contains("s3cret-token"));
    let private_then_linux = document(&[
        &format!("{}\n{PRIVATE}", rows(&[("evidence", "none")])),
        &rows(&[("responsibility", "r2-observe"), ("scope", "linux")]),
    ]);
    let hidden_linux = command
        .run(&argv(&private_then_linux))
        .expect("linux after leakage");
    assert_gap(&hidden_linux, "r2-observe", "native-request");
    assert!(!render(&hidden_linux).contains("s3cret-token"));
    let named = document(&[&rows(&[
        ("responsibility", "sim-private/land"),
        ("scope", "linux"),
    ])]);
    let named_gap = command
        .run(&argv(&named))
        .expect("redacted native responsibility");
    assert_gap(&named_gap, "", "native-request");
    let named_text = render(&named_gap);
    assert!(!named_text.contains("sim-private"));
    assert!(!named_text.contains("land"));
}

#[test]
fn private_malformed_and_later_faults_gap_the_set() {
    let command = WorldCommandLib::new().expect("command");
    let private_row = command
        .run(&argv(&document(&[&format!("{}\n{PRIVATE}", rows(&[]))])))
        .expect("private");
    assert_gap(&private_row, "", "private-leakage");
    let private_text = render(&private_row);
    assert!(!private_text.contains(PRIVATE));
    assert!(!private_text.contains("s3cret-token"));
    assert_gap(
        &command.run(&argv("")).expect("empty"),
        "",
        "missing-transfer",
    );
    assert_gap(
        &command
            .run(&argv("transfer world/responsibility-transfer-v1\n"))
            .expect("header only"),
        "",
        "missing-transfer",
    );
    assert_gap(
        &command
            .run(&argv("transfer world/other\n"))
            .expect("header"),
        "",
        "header",
    );
    let dropped = rows(&[])
        .lines()
        .filter(|line| !line.starts_with("grant "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_gap(
        &command.run(&argv(&document(&[&dropped]))).expect("grant"),
        "r3-prove",
        "grant",
    );
    assert_gap(
        &command
            .run(&argv(&document(&[&format!("{}\nnote extra", rows(&[]))])))
            .expect("field"),
        "r3-prove",
        "field",
    );
    let later = document(&[
        &rows(&[]),
        &rows(&[
            ("responsibility", "r5-land"),
            ("scope", "native"),
            ("callers", "3"),
            ("previous", "cli/main/old-land"),
            ("evidence", "caller-zero/cli/main/old-land"),
        ]),
    ]);
    let gap = command.run(&argv(&later)).expect("later");
    assert_eq!(command.run(&argv(&later)).expect("later again"), gap);
    assert_gap(&gap, "r5-land", "native-request");
    let missing = command.run(&["sim".to_owned(), "transfer".to_owned()]);
    assert!(matches!(missing, Err(WorldError::Qualification(reason)) if reason == "usage"));
    let extra = command.run(&[
        "sim".to_owned(),
        "transfer".to_owned(),
        rows(&[]),
        "tail".to_owned(),
    ]);
    assert!(matches!(extra, Err(WorldError::Qualification(reason)) if reason == "usage"));
}

fn retain(responsibility: &str, role: &str, callers: &str) -> String {
    rows(&[
        ("responsibility", responsibility),
        ("entrypoint", "cli/main/retain"),
        ("previous", role),
        ("disposition", "retain"),
        ("callers", callers),
        ("evidence", "none"),
        ("role", role),
        ("grant", "cap/retain"),
        ("provider", "model/retain"),
    ])
}

fn rows(overrides: &[(&str, &str)]) -> String {
    let mut base = [
        ("responsibility", "r3-prove"),
        ("scope", "portable"),
        ("entrypoint", "cli/main/prove"),
        ("previous", "cli/main/old-prove"),
        ("disposition", "retire"),
        ("callers", "0"),
        ("evidence", "caller-zero/cli/main/old-prove"),
        ("role", "none"),
        ("grant", "cap/prove"),
        ("provider", "model/prove"),
        ("credential", "none"),
    ];
    for (key, value) in overrides {
        let Some(slot) = base.iter_mut().find(|(name, _)| name == key) else {
            panic!("unknown override {key}");
        };
        slot.1 = value;
    }
    base.iter()
        .map(|(key, value)| format!("{key} {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn document(parts: &[&str]) -> String {
    let mut text = String::from("transfer world/responsibility-transfer-v1\n");
    for part in parts {
        text.push('\n');
        text.push_str(part);
        text.push('\n');
    }
    text
}

fn argv(document: &str) -> Vec<String> {
    vec!["sim".to_owned(), "transfer".to_owned(), document.to_owned()]
}

fn assert_gap(value: &Datum, responsibility: &str, reason: &str) {
    assert_eq!(tag(value), "world/transfer-gap-v1");
    assert_eq!(text(value, "responsibility"), responsibility);
    assert_eq!(text(value, "reason"), reason);
    assert_eq!(text(value, "seal"), "unclaimed");
    assert_eq!(count(value, "effects"), "0");
    assert_eq!(count(value, "subprocesses"), "0");
    assert!(flag(value, "resident"));
}

fn tag(value: &Datum) -> String {
    let Datum::Node { tag, .. } = value else {
        panic!("node");
    };
    tag.as_qualified_str().to_string()
}

fn field<'a>(value: &'a Datum, name: &str) -> &'a Datum {
    let Datum::Node { fields, .. } = value else {
        panic!("node");
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, item)| item)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Datum::String(text) => text.clone(),
        other => panic!("missing string {name}: {other:?}"),
    }
}

fn count(value: &Datum, name: &str) -> String {
    match field(value, name) {
        Datum::Number(number) => number.canonical.clone(),
        other => panic!("missing count {name}: {other:?}"),
    }
}

fn flag(value: &Datum, name: &str) -> bool {
    match field(value, name) {
        Datum::Bool(value) => *value,
        other => panic!("missing flag {name}: {other:?}"),
    }
}
