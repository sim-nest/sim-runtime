// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored canary record compared with the loaded command.

use std::collections::BTreeSet;

use super::{OwnedCanary, OwnedGap};

const DISCLOSURE: &str = "policy/no-v3-disclosure";
const PURPOSE: &str = "world/model-canary";
const SOURCE: &str = "git/branch/main";
const HEAD: &str = "model-commit-main";
const FRESH: &str = "git/branch/canary-1";

struct GapSpec {
    label: &'static str,
    realization: &'static str,
    reason: &'static str,
    source: &'static str,
    destination: &'static str,
    purpose: &'static str,
}

const GAPS: &[GapSpec] = &[
    GapSpec {
        label: "wrong",
        realization: "model",
        reason: "wrong-purpose",
        source: SOURCE,
        destination: "git/branch/canary-3",
        purpose: "world/deliver",
    },
    GapSpec {
        label: "spent",
        realization: "model",
        reason: "consumed-purpose",
        source: SOURCE,
        destination: "git/branch/canary-3",
        purpose: PURPOSE,
    },
    GapSpec {
        label: "branch",
        realization: "model",
        reason: "public-target",
        source: SOURCE,
        destination: SOURCE,
        purpose: PURPOSE,
    },
    GapSpec {
        label: "tag",
        realization: "model",
        reason: "public-target",
        source: SOURCE,
        destination: "git/tag/v1",
        purpose: PURPOSE,
    },
    GapSpec {
        label: "package",
        realization: "model",
        reason: "public-target",
        source: SOURCE,
        destination: "registry/checksum/sim-kernel",
        purpose: PURPOSE,
    },
    GapSpec {
        label: "merge",
        realization: "model",
        reason: "public-target",
        source: SOURCE,
        destination: "git/merge/topic",
        purpose: PURPOSE,
    },
    GapSpec {
        label: "private",
        realization: "model",
        reason: "private-leakage",
        source: SOURCE,
        destination: "git/branch/canary-5",
        purpose: "",
    },
    GapSpec {
        label: "native",
        realization: "native",
        reason: "unavailable-receiver",
        source: SOURCE,
        destination: FRESH,
        purpose: PURPOSE,
    },
    GapSpec {
        label: "real",
        realization: "real",
        reason: "unavailable-receiver",
        source: SOURCE,
        destination: FRESH,
        purpose: PURPOSE,
    },
];

pub(super) fn expected_deck() -> String {
    let mut lines = vec![
        format!("entry {}", super::ENTRY),
        "effects 0".to_owned(),
        "subprocesses 0".to_owned(),
        "resident true".to_owned(),
        canary_line(&authored_canary()),
    ];
    lines.extend(GAPS.iter().map(authored_gap));
    lines.join("\n")
}

pub(super) fn required_ids() -> BTreeSet<String> {
    let mut claims = BTreeSet::from([format!("canary {FRESH} verified dispatches 1 repeat 0")]);
    for gap in GAPS {
        claims.insert(format!("gap {} {}", gap.label, gap.reason));
    }
    claims
}

pub(super) fn canary_line(row: &OwnedCanary) -> String {
    format!(
        "canary {} {} source {} {} {} {} {} dest {} {} {} {} dispatches {} repeat {} performers {} repeat-performers {} observations {} grant {} {} disclosure {} recovered {} resident {} effects {} subprocesses {}",
        shown(&row.realization),
        shown(&row.class),
        shown(&row.source_selector),
        shown(&row.source_kind),
        shown(&row.source_state),
        shown(&row.source_provenance),
        shown(&row.source_value),
        shown(&row.destination_selector),
        shown(&row.destination_state),
        shown(&row.destination_provenance),
        shown(&row.destination_value),
        row.dispatches,
        row.repeat_dispatches,
        row.performer_calls,
        row.repeat_performer_calls,
        row.observations,
        shown(&row.grant_purpose),
        shown(&row.grant_state),
        shown(&row.disclosure),
        row.recovered,
        row.resident,
        row.effects,
        row.subprocesses,
    )
}

pub(super) fn gap_line(label: &str, row: &OwnedGap) -> String {
    format!(
        "gap {} {} {} source {} dest {} purpose {} dispatches {}",
        label,
        shown(&row.realization),
        shown(&row.reason),
        shown(&row.source),
        shown(&row.destination),
        shown(&row.purpose),
        row.dispatches,
    )
}

fn authored_canary() -> OwnedCanary {
    OwnedCanary {
        realization: "model".to_owned(),
        class: "verified".to_owned(),
        source_selector: SOURCE.to_owned(),
        source_kind: "branch".to_owned(),
        source_state: "current".to_owned(),
        source_provenance: "model/git/branch/main".to_owned(),
        source_value: HEAD.to_owned(),
        destination_selector: FRESH.to_owned(),
        destination_state: "current".to_owned(),
        destination_provenance: "model/git/branch/canary-1".to_owned(),
        destination_value: HEAD.to_owned(),
        dispatches: "1".to_owned(),
        repeat_dispatches: "0".to_owned(),
        performer_calls: "1".to_owned(),
        repeat_performer_calls: "0".to_owned(),
        observations: "3".to_owned(),
        grant_purpose: PURPOSE.to_owned(),
        grant_state: "consumed".to_owned(),
        disclosure: DISCLOSURE.to_owned(),
        recovered: "true".to_owned(),
        resident: "true".to_owned(),
        effects: "0".to_owned(),
        subprocesses: "0".to_owned(),
    }
}

fn authored_gap(spec: &GapSpec) -> String {
    gap_line(
        spec.label,
        &OwnedGap {
            realization: spec.realization.to_owned(),
            reason: spec.reason.to_owned(),
            source: spec.source.to_owned(),
            destination: spec.destination.to_owned(),
            purpose: spec.purpose.to_owned(),
            dispatches: "0".to_owned(),
        },
    )
}

fn shown(value: &str) -> String {
    if value.is_empty() {
        "blank".to_owned()
    } else if super::private(value) {
        "hidden".to_owned()
    } else {
        value.to_owned()
    }
}
