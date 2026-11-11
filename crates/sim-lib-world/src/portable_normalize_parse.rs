// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the bind normalize record.

use sim_kernel::Datum;

use super::OwnedNormalize;

pub(super) fn parsed_normalize(value: &Datum) -> Option<OwnedNormalize> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    Some(OwnedNormalize {
        case: text(value, "case")?,
        reason: text(value, "reason")?,
        alias: text(value, "alias")?,
        diagnostic: text(value, "diagnostic")?,
        field: text(value, "field")?,
        role: text(value, "role")?,
        wrong_role: text(value, "wrong-role")?,
        old_byte: text(value, "old-byte")?,
        authority: text(value, "authority")?,
    })
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
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
