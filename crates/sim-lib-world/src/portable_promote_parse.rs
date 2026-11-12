// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for promote command records.

use sim_kernel::Datum;

use super::{OwnedGap, OwnedPromote};

pub(super) fn parsed_promote(value: &Datum) -> Option<OwnedPromote> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    Some(OwnedPromote {
        case: text(value, "case")?,
        packages: number(value, "packages")?,
        needs: number(value, "needs")?,
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
