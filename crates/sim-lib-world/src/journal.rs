// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded journal command.
//!
//! One compare opens one execution and runs `rebuild` and `rebuild_report`.
//! Equal results are the same computation. A later or concurrent identical
//! case returns that record. A private value is refused before the reservation.

use std::cell::Cell;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "journal_drive.rs"]
mod drive;
#[path = "journal_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

thread_local! {
    static CALLS: Cell<u64> = const { Cell::new(0) };
}

/// Runs one named journal case through the published rebuild pair.
///
/// # Errors
/// Returns an error when the journal record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    note_call();
    if leaked(case) {
        return Ok(gap("", "private-leakage", 0, 0));
    }
    if !known(case) {
        return Ok(gap(case, "unknown-case", 0, 0));
    }
    let claim = match memory::claim(case) {
        memory::Reservation::Ready(found) => return Ok(found),
        memory::Reservation::Mine(claim) => claim,
    };
    let rendered = receive(canonical_case(case))?;
    claim.commit(&rendered);
    Ok(rendered)
}

/// Accepts the loaded compare record when its reason is the stated gap.
///
/// # Errors
/// Returns an error when the record is not the journal gap, or when its
/// reason disagrees with the phase row.
pub(crate) fn confirm(stated: &str, value: &Datum) -> Result<(), WorldError> {
    let found = reason_of(value)?;
    if found != stated {
        return Err(WorldError::Qualification(
            "journal owner disagreed with the phase gap".to_owned(),
        ));
    }
    Ok(())
}

/// Journal-command entries on this thread, including a retained record.
#[cfg(test)]
pub(crate) fn calls_on_this_thread() -> u64 {
    CALLS.with(Cell::get)
}

fn note_call() {
    CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

fn known(case: &str) -> bool {
    matches!(case, "compare") || cfg!(test) && case == "journal-memory-race"
}

fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "journal-memory-race" {
        return "compare";
    }
    case
}

fn receive(case: &str) -> Result<Datum, WorldError> {
    if case != "compare" {
        return Ok(gap(case, "unknown-case", 0, 0));
    }
    let compared = drive::compare()?;
    Ok(gap(case, compared.reason, compared.records, 2))
}

fn gap(case: &str, reason: &str, records: u64, invokes: u64) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "journal-gap-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("evidence"), string(shown(&records.to_string()))),
            (Symbol::new("invokes"), query::count_datum(invokes)),
            (Symbol::new("reason"), string(shown(reason))),
            (Symbol::new("records"), query::count_datum(records)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn reason_of(value: &Datum) -> Result<String, WorldError> {
    let Datum::Node { tag, fields } = value else {
        return Err(WorldError::Qualification(
            "journal owner returned no record".to_owned(),
        ));
    };
    if tag.as_qualified_str() != "world/journal-gap-v1" {
        return Err(WorldError::Qualification(
            "journal owner returned no gap".to_owned(),
        ));
    }
    let Some((_, field)) = fields
        .iter()
        .find(|(field, _)| field.to_string() == "reason")
    else {
        return Err(WorldError::Qualification(
            "journal owner omitted the reason".to_owned(),
        ));
    };
    match field {
        Datum::String(reason) => Ok(reason.clone()),
        _ => Err(WorldError::Qualification(
            "journal owner omitted the reason".to_owned(),
        )),
    }
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}
