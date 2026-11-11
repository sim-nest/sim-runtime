// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for delivery command records.

use sim_kernel::Datum;

use super::{OwnedGap, OwnedPlan, OwnedRemain, OwnedRow, OwnedStatus, OwnedWhy};

pub(super) fn parsed_plan(value: &Datum) -> Option<OwnedPlan> {
    if super::kind_of(value) != super::PLAN {
        return None;
    }
    Some(OwnedPlan {
        realization: text(value, "realization")?,
        omitted: rows(super::field(value, "omitted"), super::OMISSION)?,
        actions: rows(super::field(value, "actions"), super::ACTION)?,
        blocked: flag(value, "blocked")?,
        noop: flag(value, "noop")?,
        dispatches: number(value, "dispatches")?,
        performer_calls: number(value, "performer-calls")?,
        observations: number(value, "observations")?,
        effects: number(value, "effects")?,
        subprocesses: number(value, "subprocesses")?,
        recovered: flag(value, "recovered")?,
        resident: flag(value, "resident")?,
    })
}

pub(super) fn parsed_status(value: &Datum) -> Option<OwnedStatus> {
    if super::kind_of(value) != super::STATUS {
        return None;
    }
    Some(OwnedStatus {
        realization: text(value, "realization")?,
        blocked: flag(value, "blocked")?,
        noop: flag(value, "noop")?,
        dispatches: number(value, "dispatches")?,
        effects: number(value, "effects")?,
        performer_calls: number(value, "performer-calls")?,
        omitted: selectors(super::field(value, "omitted"), super::OMISSION)?,
        remaining: remains(super::field(value, "remaining"))?,
        subprocesses: number(value, "subprocesses")?,
        resident: flag(value, "resident")?,
        disclosure: text(value, "disclosure")?,
    })
}

pub(super) fn parsed_why(value: &Datum) -> Option<OwnedWhy> {
    if super::kind_of(value) != super::WHY {
        return None;
    }
    Some(OwnedWhy {
        selector: text(value, "selector")?,
        cause: text(value, "cause")?,
        class: text(value, "class")?,
        acknowledgement: text(value, "acknowledgement")?,
        dispatches: number(value, "dispatches")?,
        effects: number(value, "effects")?,
        observations: number(value, "observations")?,
        state: text(value, "state")?,
        value: super::optional_text(value, "value")?,
        resident: flag(value, "resident")?,
    })
}

pub(super) fn parsed_gap(value: &Datum) -> Option<OwnedGap> {
    if super::kind_of(value) != super::GAP {
        return None;
    }
    Some(OwnedGap {
        realization: match super::field(value, "realization") {
            Some(Datum::String(text)) => text.clone(),
            _ => return None,
        },
        reason: text(value, "reason")?,
        selector: match super::field(value, "selector") {
            Some(Datum::String(text)) => text.clone(),
            _ => return None,
        },
    })
}

fn rows(value: Option<&Datum>, kind: &str) -> Option<Vec<OwnedRow>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items.iter().map(|item| one_row(item, kind)).collect()
}

fn one_row(value: &Datum, kind: &str) -> Option<OwnedRow> {
    if super::kind_of(value) != kind {
        return None;
    }
    Some(OwnedRow {
        realization: String::new(),
        selector: text(value, "selector")?,
        cause: text(value, "cause")?,
        class: text(value, "class")?,
        acknowledgement: text(value, "acknowledgement")?,
        state: text(value, "state")?,
        dispatches: number(value, "dispatches")?,
        performer_calls: number(value, "performer-calls")?,
        observations: number(value, "observations")?,
        effects: number(value, "effects")?,
        subprocesses: number(value, "subprocesses")?,
        desired: text(value, "desired")?,
        provenance: text(value, "provenance")?,
        value: super::optional_text(value, "value")?,
    })
}

fn selectors(value: Option<&Datum>, kind: &str) -> Option<Vec<String>> {
    Some(
        rows(value, kind)?
            .into_iter()
            .map(|row| row.selector)
            .collect(),
    )
}

fn remains(value: Option<&Datum>) -> Option<Vec<OwnedRemain>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            if super::kind_of(item) != super::REMAINING {
                return None;
            }
            Some(OwnedRemain {
                selector: text(item, "selector")?,
                cause: text(item, "cause")?,
                intent: text(item, "intent")?,
            })
        })
        .collect()
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match super::field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn number(value: &Datum, name: &str) -> Option<String> {
    match super::field(value, name) {
        Some(Datum::Number(number)) if !number.canonical.is_empty() => {
            Some(number.canonical.clone())
        }
        _ => None,
    }
}

fn flag(value: &Datum, name: &str) -> Option<String> {
    match super::field_bool(value, name) {
        text if text == "missing" => None,
        text => Some(text),
    }
}
