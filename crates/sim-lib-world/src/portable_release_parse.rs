// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for release command records.

use sim_kernel::Datum;

use super::{GapRow, PlanRow, WhyRow};

pub(super) fn parsed_plan(value: &Datum) -> Option<PlanRow> {
    if super::kind_of(value) != super::PLAN {
        return None;
    }
    let realization = super::field_text(value, "realization");
    let disclosure = super::field_text(value, "disclosure");
    let noop = super::field_bool(value, "noop");
    if realization == "missing" || disclosure == "missing" || noop == "missing" {
        return None;
    }
    let actions = action_list(super::field(value, "actions"))?;
    Some(super::PlanRow {
        realization,
        omitted: omission_selectors(super::field(value, "omitted"))?,
        actions,
        dependencies: super::string_list(super::field(value, "dependencies"))?,
        capabilities: super::string_list(super::field(value, "capabilities"))?,
        noop,
        disclosure,
    })
}

pub(super) fn parsed_status(value: &Datum) -> Option<super::StatusRow> {
    if super::kind_of(value) != super::STATUS {
        return None;
    }
    let realization = super::field_text(value, "realization");
    let disclosure = super::field_text(value, "disclosure");
    let noop = super::field_bool(value, "noop");
    if realization == "missing" || disclosure == "missing" || noop == "missing" {
        return None;
    }
    Some(super::StatusRow {
        realization,
        omitted: omission_selectors(super::field(value, "omitted"))?,
        noop,
        disclosure,
        remaining: remain_list(super::field(value, "remaining"))?,
    })
}

pub(super) fn parsed_why(value: &Datum) -> Option<WhyRow> {
    if super::kind_of(value) != super::WHY {
        return None;
    }
    let selector = super::field_text(value, "selector");
    let reason = super::field_text(value, "reason");
    let intent = super::field_text(value, "intent");
    if selector == "missing" || reason == "missing" || intent == "missing" {
        return None;
    }
    Some(super::WhyRow {
        selector,
        reason,
        class: super::optional_text(value, "class")?,
        state: super::optional_text(value, "state")?,
        value: super::optional_text(value, "value")?,
        intent,
    })
}

pub(super) fn parsed_gap(value: &Datum) -> Option<GapRow> {
    if super::kind_of(value) != super::GAP {
        return None;
    }
    let realization = super::field_text(value, "realization");
    let reason = super::field_text(value, "reason");
    let selector = match super::field(value, "selector") {
        Some(Datum::String(text)) => text.clone(),
        _ => return None,
    };
    if realization == "missing" || reason == "missing" {
        return None;
    }
    Some(super::GapRow {
        realization,
        reason,
        selector,
    })
}

fn action_list(value: Option<&Datum>) -> Option<Vec<super::Act>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            if super::kind_of(item) != super::ACTION {
                return None;
            }
            let selector = super::field_text(item, "selector");
            let class = super::field_text(item, "class");
            let reason = super::field_text(item, "reason");
            let state = super::field_text(item, "state");
            let port = super::field_text(item, "port");
            let provenance = super::field_text(item, "provenance");
            let desired = super::field_text(item, "desired");
            let intent = super::field_text(item, "intent");
            let disclosure = super::field_text(item, "disclosure");
            if [
                selector.as_str(),
                class.as_str(),
                reason.as_str(),
                state.as_str(),
                port.as_str(),
                provenance.as_str(),
                desired.as_str(),
                intent.as_str(),
                disclosure.as_str(),
            ]
            .contains(&"missing")
            {
                return None;
            }
            Some(super::Act {
                selector,
                class,
                reason,
                state,
                port,
                provenance,
                value: super::optional_text(item, "value")?,
                desired,
                intent,
                disclosure,
            })
        })
        .collect()
}

fn omission_selectors(value: Option<&Datum>) -> Option<Vec<String>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            if super::kind_of(item) != super::OMISSION {
                return None;
            }
            let selector = super::field_text(item, "selector");
            if selector == "missing" {
                None
            } else {
                Some(selector)
            }
        })
        .collect()
}

pub(super) fn omission_of(value: &Datum, selector: &str) -> Option<(String, String, String)> {
    let Datum::Vector(items) = super::field(value, "omitted")? else {
        return None;
    };
    items.iter().find_map(|item| {
        if super::kind_of(item) != super::OMISSION
            || super::field_text(item, "selector") != selector
        {
            return None;
        }
        let reason = super::field_text(item, "reason");
        let intent = super::field_text(item, "intent");
        let disclosure = super::field_text(item, "disclosure");
        if reason == "missing" || intent == "missing" || disclosure == "missing" {
            None
        } else {
            Some((reason, intent, disclosure))
        }
    })
}

fn remain_list(value: Option<&Datum>) -> Option<Vec<super::Remain>> {
    let Datum::Vector(items) = value? else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            if super::kind_of(item) != super::REMAINING {
                return None;
            }
            let selector = super::field_text(item, "selector");
            let reason = super::field_text(item, "reason");
            let intent = super::field_text(item, "intent");
            if selector == "missing" || reason == "missing" || intent == "missing" {
                None
            } else {
                Some(super::Remain {
                    selector,
                    reason,
                    intent,
                })
            }
        })
        .collect()
}
