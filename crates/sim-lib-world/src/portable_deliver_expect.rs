// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored delivery record compared with the loaded command.

use std::collections::BTreeSet;

struct Row {
    selector: &'static str,
    cause: &'static str,
    class: &'static str,
    acknowledgement: &'static str,
    state: &'static str,
    dispatches: u64,
    observations: u64,
    desired: &'static str,
    provenance: &'static str,
    value: Option<&'static str>,
}

const DISCLOSURE: &str = "policy/no-v3-disclosure";

const OMITTED: &[Row] = &[
    Row {
        selector: "git/branch/main",
        cause: "already-true",
        class: "already-true",
        acknowledgement: "omitted",
        state: "current",
        dispatches: 0,
        observations: 2,
        desired: "model-commit-main",
        provenance: "model/git/branch/main",
        value: Some("model-commit-main"),
    },
    Row {
        selector: "git/direct/v1",
        cause: "already-true",
        class: "already-true",
        acknowledgement: "omitted",
        state: "current",
        dispatches: 0,
        observations: 2,
        desired: "model-tag-object-v1",
        provenance: "model/git/direct/v1",
        value: Some("model-tag-object-v1"),
    },
    Row {
        selector: "git/peeled/v1",
        cause: "already-true",
        class: "already-true",
        acknowledgement: "omitted",
        state: "current",
        dispatches: 0,
        observations: 2,
        desired: "model-commit-peeled",
        provenance: "model/git/peeled/v1",
        value: Some("model-commit-peeled"),
    },
    Row {
        selector: "git/tag/v1",
        cause: "already-true",
        class: "already-true",
        acknowledgement: "omitted",
        state: "current",
        dispatches: 0,
        observations: 2,
        desired: "model-tag-object-v1",
        provenance: "model/git/tag/v1",
        value: Some("model-tag-object-v1"),
    },
    Row {
        selector: "registry/checksum/sim-kernel",
        cause: "already-true",
        class: "already-true",
        acknowledgement: "omitted",
        state: "current",
        dispatches: 0,
        observations: 2,
        desired: "model-checksum-kernel",
        provenance: "model/registry/checksum/sim-kernel",
        value: Some("model-checksum-kernel"),
    },
];

const ACTIONS: &[Row] = &[
    Row {
        selector: "ci/sim-kernel",
        cause: "stale-observation",
        class: "uncertain",
        acknowledgement: "unsent",
        state: "stale",
        dispatches: 0,
        observations: 2,
        desired: "green",
        provenance: "model/ci/sim-kernel",
        value: Some("green"),
    },
    Row {
        selector: "ci/sim-run",
        cause: "partial-success",
        class: "uncertain",
        acknowledgement: "missing",
        state: "partial",
        dispatches: 1,
        observations: 3,
        desired: "pending",
        provenance: "model/ci/sim-run",
        value: Some("pending"),
    },
    Row {
        selector: "ci/sim-sdk",
        cause: "uncertain-reading",
        class: "uncertain",
        acknowledgement: "unsent",
        state: "uncertain",
        dispatches: 0,
        observations: 2,
        desired: "green",
        provenance: "model/ci/sim-sdk",
        value: None,
    },
    Row {
        selector: "git/branch/topic",
        cause: "divergent",
        class: "diverged",
        acknowledgement: "missing",
        state: "current",
        dispatches: 1,
        observations: 3,
        desired: "model-commit-other",
        provenance: "model/git/branch/topic",
        value: Some("model-commit-topic"),
    },
    Row {
        selector: "mirror/sim-kernel",
        cause: "registry-lag",
        class: "uncertain",
        acknowledgement: "unsent",
        state: "lagging",
        dispatches: 0,
        observations: 2,
        desired: "model-commit-topic",
        provenance: "model/mirror/sim-kernel",
        value: Some("model-commit-topic"),
    },
    Row {
        selector: "mirror/sim-private",
        cause: "denied-access",
        class: "uncertain",
        acknowledgement: "unsent",
        state: "denied",
        dispatches: 0,
        observations: 2,
        desired: "model-commit-main",
        provenance: "model/mirror/sim-private",
        value: None,
    },
];

const REFUSAL: &[(&str, &str)] = &[
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

pub(super) fn expected_deck() -> String {
    let mut lines = identity();
    lines.extend(model_block());
    lines.push(status_line("model"));
    lines.extend(ACTIONS.iter().map(remain_line));
    lines.push(why_line(&OMITTED[0]));
    lines.push(why_line(&ACTIONS[3]));
    lines.push(why_line(&ACTIONS[2]));
    lines.push(why_line(&ACTIONS[5]));
    lines.push(gap_line(
        "why-absent",
        "model",
        "selector-absent",
        "git/branch/missing",
    ));
    lines.push(gap_line("why-private", "model", "private-leakage", ""));
    lines.push(gap_line("plan-private", "model", "private-leakage", ""));
    lines.extend(refusal_block("native", "native-capability-missing"));
    lines.extend(refusal_block("real", "real-service-unbound"));
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for row in OMITTED {
        claims.insert(format!("omit {} {}", row.selector, row.cause));
    }
    for row in ACTIONS {
        claims.insert(format!("action {} {}", row.selector, row.cause));
    }
    for (selector, _) in REFUSAL {
        claims.insert(format!("native {selector} unavailable-receiver"));
        claims.insert(format!("real {selector} unavailable-receiver"));
    }
    claims.insert("why git/branch/main already-true".to_owned());
    claims.insert("why git/branch/topic divergent".to_owned());
    claims.insert("why ci/sim-sdk uncertain-reading".to_owned());
    claims.insert("why mirror/sim-private denied-access".to_owned());
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

fn model_block() -> Vec<String> {
    let mut lines = vec![plan_line("model", OMITTED, ACTIONS)];
    lines.extend(OMITTED.iter().map(|row| row_line("model", row)));
    lines.extend(ACTIONS.iter().map(|row| row_line("model", row)));
    lines
}

fn refusal_block(realization: &str, provenance: &'static str) -> Vec<String> {
    let rows = REFUSAL
        .iter()
        .map(|(selector, desired)| Row {
            selector,
            cause: "unavailable-receiver",
            class: "uncertain",
            acknowledgement: "unsent",
            state: "unavailable",
            dispatches: 0,
            observations: 2,
            desired,
            provenance,
            value: None,
        })
        .collect::<Vec<_>>();
    let mut lines = vec![plan_line(realization, &[], &rows)];
    lines.extend(rows.iter().map(|row| row_line(realization, row)));
    lines
}

fn plan_line(realization: &str, omitted: &[Row], actions: &[Row]) -> String {
    let selectors = omitted
        .iter()
        .map(|row| visible(row.selector))
        .collect::<Vec<_>>();
    let pairs = actions
        .iter()
        .map(|row| format!("{}:{}", visible(row.selector), visible(row.cause)))
        .collect::<Vec<_>>();
    let rows = omitted.iter().chain(actions.iter());
    let dispatches: u64 = rows.clone().map(|row| row.dispatches).sum();
    let observations: u64 = rows.map(|row| row.observations).sum();
    format!(
        "plan {} omitted {} actions {} blocked {} noop {} dispatches {dispatches} performer {dispatches} observations {observations} effects 0 subprocesses 0 recovered true resident true",
        visible(realization),
        counted(&selectors),
        counted(&pairs),
        if actions.is_empty() { "false" } else { "true" },
        if actions.is_empty() { "true" } else { "false" },
    )
}

fn row_line(realization: &str, row: &Row) -> String {
    format!(
        "row {} {} {} {} {} {} dispatches {} performer {} observations {} effects 0 subprocesses 0 desired {} provenance {} value {}",
        visible(realization),
        visible(row.selector),
        visible(row.cause),
        visible(row.class),
        visible(row.acknowledgement),
        visible(row.state),
        row.dispatches,
        row.dispatches,
        row.observations,
        shown(row.desired),
        shown(row.provenance),
        counted_one(row.value),
    )
}

fn status_line(realization: &str) -> String {
    let omitted = OMITTED
        .iter()
        .map(|row| visible(row.selector))
        .collect::<Vec<_>>();
    let dispatches: u64 = ACTIONS.iter().map(|row| row.dispatches).sum();
    format!(
        "status {} blocked true noop false dispatches {dispatches} effects {dispatches} performer {dispatches} omitted {} remaining {} subprocesses 0 resident true disclosure {}",
        visible(realization),
        counted(&omitted),
        ACTIONS.len(),
        shown(DISCLOSURE),
    )
}

fn remain_line(row: &Row) -> String {
    format!(
        "remain {} {} {}",
        visible(row.selector),
        visible(row.cause),
        shown(&format!("intent/reconcile/{}", row.selector)),
    )
}

fn why_line(row: &Row) -> String {
    format!(
        "why {} {} {} {} dispatches {} effects {} observations {} state {} value {} resident true",
        visible(row.selector),
        visible(row.cause),
        visible(row.class),
        visible(row.acknowledgement),
        row.dispatches,
        row.dispatches,
        row.observations,
        shown(row.state),
        counted_one(row.value),
    )
}

pub(super) fn gap_line(label: &str, realization: &str, reason: &str, selector: &str) -> String {
    format!(
        "gap {label} {} {} selector {}",
        shown(realization),
        shown(reason),
        shown(selector),
    )
}

pub(super) fn row_line_owned(row: &super::OwnedRow) -> String {
    format!(
        "row {} {} {} {} {} {} dispatches {} performer {} observations {} effects {} subprocesses {} desired {} provenance {} value {}",
        visible(&row.realization),
        visible(&row.selector),
        visible(&row.cause),
        visible(&row.class),
        visible(&row.acknowledgement),
        visible(&row.state),
        row.dispatches,
        row.performer_calls,
        row.observations,
        row.effects,
        row.subprocesses,
        shown(&row.desired),
        shown(&row.provenance),
        counted_one(row.value.as_deref()),
    )
}

pub(super) fn plan_line_owned(plan: &super::OwnedPlan) -> String {
    let omitted = plan
        .omitted
        .iter()
        .map(|row| visible(&row.selector))
        .collect::<Vec<_>>();
    let actions = plan
        .actions
        .iter()
        .map(|row| format!("{}:{}", visible(&row.selector), visible(&row.cause)))
        .collect::<Vec<_>>();
    format!(
        "plan {} omitted {} actions {} blocked {} noop {} dispatches {} performer {} observations {} effects {} subprocesses {} recovered {} resident {}",
        visible(&plan.realization),
        counted(&omitted),
        counted(&actions),
        plan.blocked,
        plan.noop,
        plan.dispatches,
        plan.performer_calls,
        plan.observations,
        plan.effects,
        plan.subprocesses,
        plan.recovered,
        plan.resident,
    )
}

pub(super) fn status_line_owned(status: &super::OwnedStatus) -> String {
    format!(
        "status {} blocked {} noop {} dispatches {} effects {} performer {} omitted {} remaining {} subprocesses {} resident {} disclosure {}",
        visible(&status.realization),
        status.blocked,
        status.noop,
        status.dispatches,
        status.effects,
        status.performer_calls,
        counted(
            &status
                .omitted
                .iter()
                .map(|selector| visible(selector))
                .collect::<Vec<_>>(),
        ),
        status.remaining.len(),
        status.subprocesses,
        status.resident,
        shown(&status.disclosure),
    )
}

pub(super) fn remain_line_owned(row: &super::OwnedRemain) -> String {
    format!(
        "remain {} {} {}",
        visible(&row.selector),
        visible(&row.cause),
        shown(&row.intent),
    )
}

pub(super) fn why_line_owned(row: &super::OwnedWhy) -> String {
    format!(
        "why {} {} {} {} dispatches {} effects {} observations {} state {} value {} resident {}",
        visible(&row.selector),
        visible(&row.cause),
        visible(&row.class),
        visible(&row.acknowledgement),
        row.dispatches,
        row.effects,
        row.observations,
        shown(&row.state),
        counted_one(row.value.as_deref()),
        row.resident,
    )
}

fn counted(values: &[String]) -> String {
    if values.is_empty() {
        "0".to_owned()
    } else {
        format!("{}|{}", values.len(), values.join("|"))
    }
}

fn counted_one(value: Option<&str>) -> String {
    match value {
        Some(text) if super::private(text) => "hidden".to_owned(),
        Some(text) => format!("1|{text}"),
        None => "0".to_owned(),
    }
}

fn visible(value: &str) -> String {
    if super::private(value) {
        "hidden".to_owned()
    } else {
        value.to_owned()
    }
}

fn shown(value: &str) -> String {
    if value.is_empty() {
        "1|".to_owned()
    } else if super::private(value) {
        "hidden".to_owned()
    } else {
        format!("1|{value}")
    }
}
