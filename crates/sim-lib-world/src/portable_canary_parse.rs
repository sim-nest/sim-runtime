// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for canary command records.

use sim_kernel::Datum;

use super::{OwnedCanary, OwnedGap};

pub(super) fn parsed_canary(value: &Datum) -> Option<OwnedCanary> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    let source = nested(value, "source", super::SOURCE)?;
    let destination = nested(value, "destination", super::BRANCH)?;
    Some(OwnedCanary {
        realization: text(value, "realization")?,
        class: text(value, "class")?,
        source_selector: text(source, "selector")?,
        source_kind: text(source, "kind")?,
        source_state: text(source, "state")?,
        source_provenance: text(source, "provenance")?,
        source_value: text(source, "value")?,
        destination_selector: text(destination, "selector")?,
        destination_state: text(destination, "state")?,
        destination_provenance: text(destination, "provenance")?,
        destination_value: text(destination, "value")?,
        dispatches: number(value, "dispatches")?,
        repeat_dispatches: number(value, "repeat-dispatches")?,
        performer_calls: number(value, "performer-calls")?,
        repeat_performer_calls: number(value, "repeat-performer-calls")?,
        observations: number(value, "observations")?,
        grant_purpose: text(value, "grant-purpose")?,
        grant_state: text(value, "grant-state")?,
        disclosure: text(value, "disclosure")?,
        recovered: flag(value, "recovered")?,
        resident: flag(value, "resident")?,
        effects: number(value, "effects")?,
        subprocesses: number(value, "subprocesses")?,
    })
}

pub(super) fn parsed_gap(value: &Datum) -> Option<OwnedGap> {
    if super::kind_of(value) != super::GAP {
        return None;
    }
    Some(OwnedGap {
        realization: text(value, "realization")?,
        reason: text(value, "reason")?,
        source: text(value, "source")?,
        destination: text(value, "destination")?,
        purpose: text(value, "purpose")?,
        dispatches: number(value, "dispatches")?,
    })
}

fn nested<'a>(value: &'a Datum, name: &str, tag: &str) -> Option<&'a Datum> {
    let nested = super::field(value, name)?;
    if super::kind_of(nested) == tag {
        Some(nested)
    } else {
        None
    }
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
    match super::field(value, name) {
        Some(Datum::Bool(true)) => Some("true".to_owned()),
        Some(Datum::Bool(false)) => Some("false".to_owned()),
        _ => None,
    }
}
