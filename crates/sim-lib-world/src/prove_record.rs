// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

use super::{EXCLUDED, PAGE};

pub(super) fn plan_record(
    leaves: &[String],
    subject: &str,
    page: usize,
    pages: usize,
) -> Result<Datum, WorldError> {
    let start = page.saturating_mul(PAGE);
    let end = (start + PAGE).min(leaves.len());
    let mut fields = vec![
        (
            "denominator",
            num(u64::try_from(leaves.len()).unwrap_or(u64::MAX)),
        ),
        ("page", num(u64::try_from(page).unwrap_or(u64::MAX))),
        ("pages", num(u64::try_from(pages).unwrap_or(u64::MAX))),
        ("leaves", list(leaves[start..end].iter().cloned())),
        ("excluded", list(EXCLUDED)),
        ("effects", num(0)),
        ("subprocesses", num(0)),
        ("owner-commands", num(0)),
        ("resident", flag(true)),
    ];
    if page + 1 < pages {
        fields.push(("continuation", text(super::plan_token(subject, page + 1)?)));
    }
    Ok(node("plan-v1", fields))
}

pub(super) fn gap(call: &str, reason: &str) -> Datum {
    node(
        "gap-v1",
        vec![
            ("call", text(call)),
            ("reason", text(reason)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
        ],
    )
}

pub(super) fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("prove", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

pub(super) fn num(value: u64) -> Datum {
    query::count_datum(value)
}

pub(super) fn flag(value: bool) -> Datum {
    Datum::Bool(value)
}

pub(super) fn text(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}

pub(super) fn list(values: impl IntoIterator<Item = impl Into<String>>) -> Datum {
    Datum::Vector(
        values
            .into_iter()
            .map(|value| Datum::String(value.into()))
            .collect(),
    )
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
