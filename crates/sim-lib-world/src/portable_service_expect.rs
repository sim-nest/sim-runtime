// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored service record compared with the loaded command.

use std::collections::BTreeSet;

pub(super) struct Reading {
    pub(super) selector: &'static str,
    state: &'static str,
    port: &'static str,
    value: Option<&'static str>,
}

struct Kept {
    selector: &'static str,
    class: &'static str,
    state: &'static str,
    port: &'static str,
    value: Option<&'static str>,
    desired: &'static str,
}

pub(super) const PROBE: &str = "git/branch/topic";

pub(super) const ROWS: &[Reading] = &[
    Reading {
        selector: "git/branch/main",
        state: "current",
        port: "git",
        value: Some("model-commit-main"),
    },
    Reading {
        selector: "git/branch/topic",
        state: "current",
        port: "git",
        value: Some("model-commit-topic"),
    },
    Reading {
        selector: "git/tag/v1",
        state: "current",
        port: "git",
        value: Some("model-tag-object-v1"),
    },
    Reading {
        selector: "git/direct/v1",
        state: "current",
        port: "git",
        value: Some("model-tag-object-v1"),
    },
    Reading {
        selector: "git/peeled/v1",
        state: "current",
        port: "git",
        value: Some("model-commit-peeled"),
    },
    Reading {
        selector: "registry/checksum/sim-kernel",
        state: "current",
        port: "registry",
        value: Some("model-checksum-kernel"),
    },
    Reading {
        selector: "ci/sim-kernel",
        state: "stale",
        port: "ci",
        value: Some("green"),
    },
    Reading {
        selector: "ci/sim-run",
        state: "partial",
        port: "ci",
        value: Some("pending"),
    },
    Reading {
        selector: "ci/sim-sdk",
        state: "uncertain",
        port: "ci",
        value: None,
    },
    Reading {
        selector: "mirror/sim-kernel",
        state: "lagging",
        port: "mirror",
        value: Some("model-commit-topic"),
    },
    Reading {
        selector: "mirror/sim-private",
        state: "denied",
        port: "mirror",
        value: None,
    },
];

const OMITTED: &[&str] = &[
    "git/branch/main",
    "git/direct/v1",
    "git/peeled/v1",
    "git/tag/v1",
    "registry/checksum/sim-kernel",
];

const ACTIONS: &[(&str, &str)] = &[("git/branch/topic", "divergent")];

const KEPT: &[Kept] = &[
    Kept {
        selector: "ci/sim-kernel",
        class: "unknown",
        state: "stale",
        port: "ci",
        value: Some("green"),
        desired: "green",
    },
    Kept {
        selector: "ci/sim-run",
        class: "unknown",
        state: "partial",
        port: "ci",
        value: Some("pending"),
        desired: "pending",
    },
    Kept {
        selector: "ci/sim-sdk",
        class: "unknown",
        state: "uncertain",
        port: "ci",
        value: None,
        desired: "green",
    },
    Kept {
        selector: "git/branch/topic",
        class: "divergent",
        state: "current",
        port: "git",
        value: Some("model-commit-topic"),
        desired: "model-commit-other",
    },
    Kept {
        selector: "mirror/sim-kernel",
        class: "unknown",
        state: "lagging",
        port: "mirror",
        value: Some("model-commit-topic"),
        desired: "model-commit-topic",
    },
    Kept {
        selector: "mirror/sim-private",
        class: "unknown",
        state: "denied",
        port: "mirror",
        value: None,
        desired: "model-commit-main",
    },
];

const PLAN_READS: &[&str] = &[
    "ci/sim-kernel",
    "ci/sim-run",
    "ci/sim-sdk",
    "git/branch/main",
    "git/branch/topic",
    "git/direct/v1",
    "git/peeled/v1",
    "git/tag/v1",
    "mirror/sim-kernel",
    "mirror/sim-private",
    "registry/checksum/sim-kernel",
];

const PLAN_PORTS: &[&str] = &["ci", "git", "mirror", "registry"];

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

pub(super) fn expected_deck() -> String {
    let mut lines = identity_lines();
    for reading in ROWS {
        lines.push(authored_row("model", reading));
    }
    lines.push(authored_plan(
        "model", OMITTED, ACTIONS, PLAN_READS, PLAN_PORTS,
    ));
    lines.extend(KEPT.iter().map(|fact| authored_kept("model", fact)));
    lines.push(authored_refusal_row("native", "native-capability-missing"));
    lines.push(authored_refusal_row("real", "real-service-unbound"));
    lines.push(authored_plan("native", &[], &[], &[], &[]));
    lines.extend(REFUSAL_DESIRED.iter().map(|(selector, desired)| {
        refusal_kept(
            "native",
            "native",
            "native-capability-missing",
            selector,
            desired,
        )
    }));
    lines.push(authored_plan("real", &[], &[], &[], &[]));
    lines.extend(REFUSAL_DESIRED.iter().map(|(selector, desired)| {
        refusal_kept("real", "real", "real-service-unbound", selector, desired)
    }));
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    for reading in ROWS {
        claims.insert(format!("state {} {}", reading.selector, reading.state));
    }
    for selector in OMITTED {
        claims.insert(format!("omitted {selector}"));
    }
    for (selector, class) in ACTIONS {
        claims.insert(format!("action {selector} {class}"));
    }
    for fact in KEPT {
        claims.insert(format!("kept {} {}", fact.selector, fact.class));
    }
    claims.insert(format!("native {PROBE} unavailable"));
    claims.insert(format!("real {PROBE} unavailable"));
    claims
}

fn authored_row(realization: &str, reading: &Reading) -> String {
    format!(
        "row {realization} {} state {} port {} provenance {} value {} reads {}",
        reading.selector,
        reading.state,
        super::counted_str(reading.port),
        super::counted_str(&format!("model/{}", reading.selector)),
        super::counted_one(reading.value),
        super::counted_str(reading.selector)
    )
}

fn authored_refusal_row(realization: &str, provenance: &str) -> String {
    format!(
        "row {realization} {PROBE} state unavailable port {} provenance {} value 0 reads 0",
        super::counted_str(realization),
        super::counted_str(provenance)
    )
}

fn authored_plan(
    realization: &str,
    omitted: &[&str],
    actions: &[(&str, &str)],
    reads: &[&str],
    ports: &[&str],
) -> String {
    format!(
        "plan {realization} omitted {} actions {} reads {} ports {}",
        super::counted(&owned(omitted)),
        action_token(actions),
        super::counted(&owned(reads)),
        super::counted(&owned(ports))
    )
}

fn authored_kept(realization: &str, fact: &Kept) -> String {
    format!(
        "kept {realization} {} class {} state {} port {} provenance {} value {} desired {}",
        fact.selector,
        fact.class,
        fact.state,
        super::counted_str(fact.port),
        super::counted_str(&format!("model/{}", fact.selector)),
        super::counted_one(fact.value),
        super::counted_str(fact.desired)
    )
}

fn refusal_kept(
    realization: &str,
    port: &str,
    provenance: &str,
    selector: &str,
    desired: &str,
) -> String {
    format!(
        "kept {realization} {selector} class unknown state unavailable port {} provenance {} value 0 desired {}",
        super::counted_str(port),
        super::counted_str(provenance),
        super::counted_str(desired)
    )
}

fn identity_lines() -> Vec<String> {
    vec![
        format!("entry {}", super::ENTRY),
        format!("kind {}", super::SNAPSHOT),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
    ]
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn action_token(actions: &[(&str, &str)]) -> String {
    if actions.is_empty() {
        "0".to_owned()
    } else {
        let parts = actions
            .iter()
            .map(|(selector, class)| format!("{selector}:{class}"))
            .collect::<Vec<_>>();
        format!("{}|{}", parts.len(), parts.join("|"))
    }
}
