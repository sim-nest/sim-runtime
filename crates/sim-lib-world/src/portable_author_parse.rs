// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for author command records.

use sim_kernel::Datum;

use super::{OwnedAuthor, OwnedGap};

pub(super) fn parsed_author(value: &Datum) -> Option<OwnedAuthor> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    Some(OwnedAuthor {
        case: text(value, "case")?,
        class: text(value, "class")?,
        members: number(value, "members")?,
        shown: text(value, "shown")?,
        lint: text(value, "lint")?,
        selection: text(value, "selection")?,
        changed: text(value, "changed")?,
        round_trip: text(value, "round-trip")?,
        shapes: text(value, "shapes")?,
        compiled: text(value, "compiled")?,
        consultation: number(value, "consultation")?,
        bytes: number(value, "bytes")?,
        proof: text(value, "proof")?,
        phase: text(value, "phase")?,
        uncertain: text(value, "uncertain")?,
        closed: text(value, "closed")?,
        grounded: text(value, "grounded")?,
        claims: number(value, "claims")?,
        entries: number(value, "entries")?,
        postimage: number(value, "postimage")?,
    })
}

pub(super) fn parsed_gap(value: &Datum) -> Option<OwnedGap> {
    if super::kind_of(value) != super::GAP {
        return None;
    }
    Some(OwnedGap {
        case: text(value, "case")?,
        reason: text(value, "reason")?,
        evidence: text(value, "evidence")?,
    })
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
