// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Field readers for a loaded work command record.

use sim_kernel::Datum;

use crate::WorldError;

pub(super) const MARKER: &str = "private-material:probe";

pub(super) fn refuse_copied(value: &Datum) -> Result<(), WorldError> {
    if contains_marker(value) {
        Err(fault("copied-marker"))
    } else if contains_claim(value) {
        Err(fault("claim-label"))
    } else {
        Ok(())
    }
}

pub(super) fn kind_of(value: &Datum) -> String {
    match value {
        Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}

pub(super) fn text(value: &Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(Datum::String(text)) => Ok(text.clone()),
        _ => Err(fault(name)),
    }
}

pub(super) fn number(value: &Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(Datum::Number(number)) if !number.canonical.is_empty() => Ok(number.canonical.clone()),
        _ => Err(fault(name)),
    }
}

pub(super) fn text_bool(value: &Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(Datum::Bool(true)) => Ok("true".to_owned()),
        Some(Datum::Bool(false)) => Ok("false".to_owned()),
        _ => Err(fault(name)),
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub(super) fn fault(detail: &str) -> WorldError {
    WorldError::Qualification(detail.to_owned())
}

fn contains_marker(value: &Datum) -> bool {
    walk_text(value, &|text| leaked(text))
}

fn contains_claim(value: &Datum) -> bool {
    walk_text(value, &|text| text.contains("completion/claim-"))
}

fn leaked(value: &str) -> bool {
    value.contains(MARKER)
        || value.contains("bo-private")
        || value.contains("docs/private")
        || value.contains("/home/")
        || value.contains("sim-private/")
}

fn walk_text(value: &Datum, probe: &dyn Fn(&str) -> bool) -> bool {
    match value {
        Datum::String(text) => probe(text),
        Datum::Symbol(symbol) => probe(&symbol.to_string()),
        Datum::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => probe(text),
            Err(_) => false,
        },
        Datum::List(items) | Datum::Vector(items) | Datum::Set(items) => {
            items.iter().any(|item| walk_text(item, probe))
        }
        Datum::Map(pairs) => pairs
            .iter()
            .any(|(key, field)| walk_text(key, probe) || walk_text(field, probe)),
        Datum::Node { tag, fields } => {
            probe(&tag.to_string())
                || fields
                    .iter()
                    .any(|(name, field)| probe(&name.to_string()) || walk_text(field, probe))
        }
        Datum::Nil | Datum::Bool(_) | Datum::Number(_) => false,
    }
}

fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, field)| field)
}
