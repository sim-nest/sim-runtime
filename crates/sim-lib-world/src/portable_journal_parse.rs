// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the journal replay record.

use sim_kernel::Datum;

use super::OwnedReplay;

pub(super) fn parsed_replay(value: &Datum) -> Option<OwnedReplay> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    Some(OwnedReplay {
        case: text(value, "case")?,
        reason: text(value, "reason")?,
        head: text(value, "head")?,
        corrupt: text(value, "corrupt")?,
        records: number(value, "records")?,
    })
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn number(value: &Datum, name: &str) -> Option<String> {
    match field(value, name) {
        Some(Datum::Number(number)) if !number.canonical.is_empty() => {
            Some(number.canonical.clone())
        }
        _ => None,
    }
}

fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, item)| item)
}
