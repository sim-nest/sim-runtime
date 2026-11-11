// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored release record compared with the loaded command.

use std::collections::BTreeSet;

struct Action {
    selector: &'static str,
    class: &'static str,
    reason: &'static str,
    state: &'static str,
    port: &'static str,
    provenance: &'static str,
    value: Option<&'static str>,
    desired: &'static str,
}

const DISCLOSURE: &str = "policy/no-v3-disclosure";
const MODEL_POLICY: &str = "policy/explicit-model-v1";
const NATIVE_POLICY: &str = "policy/no-native-fallback-v1";
const REAL_POLICY: &str = "policy/no-real-service-fallback-v1";
const NATIVE_PROVENANCE: &str = "native-capability-missing";
const REAL_PROVENANCE: &str = "real-service-unbound";

const OMITTED: &[&str] = &[
    "git/branch/main",
    "git/direct/v1",
    "git/peeled/v1",
    "git/tag/v1",
    "registry/checksum/sim-kernel",
];

const MODEL_ACTIONS: &[Action] = &[
    Action {
        selector: "ci/sim-kernel",
        class: "unknown",
        reason: "original-preserved",
        state: "stale",
        port: "ci",
        provenance: "model/ci/sim-kernel",
        value: Some("green"),
        desired: "green",
    },
    Action {
        selector: "ci/sim-run",
        class: "unknown",
        reason: "original-preserved",
        state: "partial",
        port: "ci",
        provenance: "model/ci/sim-run",
        value: Some("pending"),
        desired: "pending",
    },
    Action {
        selector: "ci/sim-sdk",
        class: "unknown",
        reason: "original-preserved",
        state: "uncertain",
        port: "ci",
        provenance: "model/ci/sim-sdk",
        value: None,
        desired: "green",
    },
    Action {
        selector: "git/branch/topic",
        class: "divergent",
        reason: "desired-differs",
        state: "current",
        port: "git",
        provenance: "model/git/branch/topic",
        value: Some("model-commit-topic"),
        desired: "model-commit-other",
    },
    Action {
        selector: "mirror/sim-kernel",
        class: "unknown",
        reason: "original-preserved",
        state: "lagging",
        port: "mirror",
        provenance: "model/mirror/sim-kernel",
        value: Some("model-commit-topic"),
        desired: "model-commit-topic",
    },
    Action {
        selector: "mirror/sim-private",
        class: "unknown",
        reason: "original-preserved",
        state: "denied",
        port: "mirror",
        provenance: "model/mirror/sim-private",
        value: None,
        desired: "model-commit-main",
    },
];

const REFUSAL_DESIRED: &[(&str, &str)] = &[
    ("ci/sim-kernel", "green"),
    ("ci/sim-run", "pending"),
    ("ci/sim-sdk", "green"),
    ("git/branch/main", "model-commit-main"),
    ("git/branch/topic", "model-commit-other"),
    ("git/direct/v1", "model-tag-object-v1"),
    ("git/peeled/v1", "model-commit-peeled"),
    ("git/tag/v1", "model-tag-object-v1"),
    ("mirror/sim-kernel", "model-commit-topic"),
    ("mirror/sim-private", "model-commit-main"),
    ("registry/checksum/sim-kernel", "model-checksum-kernel"),
];

const MODEL_DEPS: &[&str] = &["ci", "git", "mirror", "registry"];

pub(super) fn expected_deck() -> String {
    let mut lines = identity();
    lines.extend(plan_block(
        "model",
        OMITTED,
        MODEL_ACTIONS,
        MODEL_DEPS,
        &[MODEL_POLICY],
        false,
    ));
    lines.push(authored_status("model", MODEL_ACTIONS, OMITTED, false));
    lines.extend(MODEL_ACTIONS.iter().map(authored_remain));
    lines.push(authored_why(
        "git/branch/main",
        "completed",
        None,
        None,
        None,
    ));
    lines.push(authored_why(
        "git/branch/topic",
        "desired-differs",
        Some("divergent"),
        Some("current"),
        Some("model-commit-topic"),
    ));
    lines.push(authored_why(
        "ci/sim-sdk",
        "original-preserved",
        Some("unknown"),
        Some("uncertain"),
        None,
    ));
    lines.push(authored_gap(
        "why-absent",
        "model",
        "selector-absent",
        "git/branch/missing",
    ));
    lines.push(authored_gap("why-private", "model", "private-leakage", ""));
    lines.push(authored_gap("plan-private", "model", "private-leakage", ""));
    lines.extend(refusal_block(
        "native",
        "native",
        NATIVE_POLICY,
        NATIVE_PROVENANCE,
    ));
    lines.extend(refusal_block("real", "real", REAL_POLICY, REAL_PROVENANCE));
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for selector in OMITTED {
        claims.insert(format!("omit {selector}"));
    }
    for action in MODEL_ACTIONS {
        claims.insert(format!("action {} {}", action.selector, action.reason));
    }
    for (selector, _) in REFUSAL_DESIRED {
        claims.insert(format!("native {selector} original-preserved"));
        claims.insert(format!("real {selector} original-preserved"));
    }
    claims.insert("why git/branch/main completed".to_owned());
    claims.insert("why git/branch/topic desired-differs".to_owned());
    claims.insert("why ci/sim-sdk original-preserved".to_owned());
    claims.insert("gap git/branch/missing selector-absent".to_owned());
    claims.insert("gap private private-leakage".to_owned());
    claims
}

fn identity() -> Vec<String> {
    vec![
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
    ]
}

fn plan_block(
    realization: &str,
    omitted: &[&str],
    actions: &[Action],
    deps: &[&str],
    caps: &[&str],
    noop: bool,
) -> Vec<String> {
    let mut lines = vec![plan_line(
        realization,
        &owned(omitted),
        &actions
            .iter()
            .map(|action| (action.selector.to_owned(), action.reason.to_owned()))
            .collect::<Vec<_>>(),
        &owned(deps),
        &owned(caps),
        noop,
        DISCLOSURE,
    )];
    lines.extend(omitted.iter().map(|selector| {
        omit_line(
            realization,
            selector,
            "completed",
            &intent(selector),
            DISCLOSURE,
        )
    }));
    lines.extend(
        actions
            .iter()
            .map(|action| action_line(realization, action)),
    );
    lines
}

fn refusal_block(
    realization: &str,
    port: &'static str,
    policy: &str,
    provenance: &'static str,
) -> Vec<String> {
    let actions = REFUSAL_DESIRED
        .iter()
        .map(|(selector, desired)| Action {
            selector,
            class: "unknown",
            reason: "original-preserved",
            state: "unavailable",
            port,
            provenance,
            value: None,
            desired,
        })
        .collect::<Vec<_>>();
    plan_block(realization, &[], &actions, &[], &[policy], false)
}

fn action_line(realization: &str, action: &Action) -> String {
    let intent = intent(action.selector);
    act_line(&ActLine {
        realization,
        selector: action.selector,
        class: action.class,
        reason: action.reason,
        state: action.state,
        port: action.port,
        provenance: action.provenance,
        value: action.value,
        desired: action.desired,
        intent: &intent,
        disclosure: DISCLOSURE,
    })
}

fn authored_status(realization: &str, actions: &[Action], omitted: &[&str], noop: bool) -> String {
    status_line(
        realization,
        &actions
            .iter()
            .map(|action| (action.selector.to_owned(), action.reason.to_owned()))
            .collect::<Vec<_>>(),
        &owned(omitted),
        noop,
        DISCLOSURE,
    )
}

fn authored_remain(action: &Action) -> String {
    remain_line(action.selector, action.reason, &intent(action.selector))
}

fn authored_why(
    selector: &str,
    reason: &str,
    class: Option<&str>,
    state: Option<&str>,
    value: Option<&str>,
) -> String {
    why_line(selector, reason, class, state, value, &intent(selector))
}

fn authored_gap(label: &str, realization: &str, reason: &str, selector: &str) -> String {
    gap_line(label, realization, reason, selector)
}

fn intent(selector: &str) -> String {
    format!("intent/reconcile/{selector}")
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().copied().map(str::to_owned).collect()
}

pub(super) fn plan_line(
    realization: &str,
    omitted: &[String],
    actions: &[(String, String)],
    dependencies: &[String],
    capabilities: &[String],
    noop: bool,
    disclosure: &str,
) -> String {
    let action_token = if actions.is_empty() {
        "0".to_owned()
    } else {
        let parts = actions
            .iter()
            .map(|(selector, reason)| format!("{selector}:{reason}"))
            .collect::<Vec<_>>();
        format!("{}|{}", parts.len(), parts.join("|"))
    };
    format!(
        "plan {realization} omitted {} actions {action_token} deps {} caps {} noop {} disclosure {}",
        counted(omitted),
        counted(dependencies),
        counted(capabilities),
        if noop { "true" } else { "false" },
        shown(disclosure)
    )
}

pub(super) fn omit_line(
    realization: &str,
    selector: &str,
    reason: &str,
    intent: &str,
    disclosure: &str,
) -> String {
    format!(
        "omit {realization} {selector} {reason} {} {}",
        shown(intent),
        shown(disclosure)
    )
}

pub(super) struct ActLine<'a> {
    pub(super) realization: &'a str,
    pub(super) selector: &'a str,
    pub(super) class: &'a str,
    pub(super) reason: &'a str,
    pub(super) state: &'a str,
    pub(super) port: &'a str,
    pub(super) provenance: &'a str,
    pub(super) value: Option<&'a str>,
    pub(super) desired: &'a str,
    pub(super) intent: &'a str,
    pub(super) disclosure: &'a str,
}

pub(super) fn act_line(row: &ActLine<'_>) -> String {
    format!(
        "act {} {} {} {} {} {} {} {} {} {} {}",
        row.realization,
        row.selector,
        row.class,
        row.reason,
        row.state,
        shown(row.port),
        shown(row.provenance),
        counted_one(row.value),
        shown(row.desired),
        shown(row.intent),
        shown(row.disclosure)
    )
}

pub(super) fn status_line(
    realization: &str,
    remaining: &[(String, String)],
    omitted: &[String],
    noop: bool,
    disclosure: &str,
) -> String {
    let token = if remaining.is_empty() {
        "0".to_owned()
    } else {
        let parts = remaining
            .iter()
            .map(|(selector, reason)| format!("{selector}:{reason}"))
            .collect::<Vec<_>>();
        format!("{}|{}", parts.len(), parts.join("|"))
    };
    format!(
        "status {realization} remaining {token} omitted {} noop {} disclosure {}",
        counted(omitted),
        if noop { "true" } else { "false" },
        shown(disclosure)
    )
}

pub(super) fn remain_line(selector: &str, reason: &str, intent: &str) -> String {
    format!("remain {selector} {reason} {}", shown(intent))
}

pub(super) fn why_line(
    selector: &str,
    reason: &str,
    class: Option<&str>,
    state: Option<&str>,
    value: Option<&str>,
    intent: &str,
) -> String {
    format!(
        "why {selector} {reason} class {} state {} value {} intent {}",
        shown_opt(class),
        shown_opt(state),
        counted_one(value),
        shown(intent)
    )
}

pub(super) fn gap_line(label: &str, realization: &str, reason: &str, selector: &str) -> String {
    format!(
        "gap {label} {} {} selector {}",
        shown(realization),
        shown(reason),
        shown(selector)
    )
}
fn counted(values: &[String]) -> String {
    if values.is_empty() {
        "0".to_owned()
    } else {
        format!("{}|{}", values.len(), values.join("|"))
    }
}

fn counted_str(value: &str) -> String {
    format!("1|{value}")
}

fn counted_one(value: Option<&str>) -> String {
    match value {
        Some(text) if super::private(text) => "hidden".to_owned(),
        Some(text) => counted_str(text),
        None => "0".to_owned(),
    }
}

fn shown(value: &str) -> String {
    if super::private(value) {
        "hidden".to_owned()
    } else {
        counted_str(value)
    }
}

fn shown_opt(value: Option<&str>) -> String {
    match value {
        Some(text) => shown(text),
        None => "0".to_owned(),
    }
}
