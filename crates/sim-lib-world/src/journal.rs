// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded journal command.
//!
//! One compare opens one execution and runs `rebuild` and `rebuild_report`.
//! Equal results are the same computation. Replay exports that journal's
//! stored state and reopens it on a second instance. A later or concurrent
//! identical case returns that record. A private value is refused before
//! the reservation.

use std::cell::Cell;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "journal_drive.rs"]
mod drive;
#[path = "journal_memory.rs"]
mod memory;
#[path = "journal_replay.rs"]
mod replay;

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

/// Journal-command entries on this thread, including a retained record.
#[cfg(test)]
pub(crate) fn calls_on_this_thread() -> u64 {
    CALLS.with(Cell::get)
}

fn note_call() {
    CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

fn known(case: &str) -> bool {
    matches!(case, "compare" | "replay") || cfg!(test) && case == "journal-memory-race"
}

fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "journal-memory-race" {
        return "compare";
    }
    case
}

fn receive(case: &str) -> Result<Datum, WorldError> {
    if case == "replay" {
        return Ok(replay_record(&replay::reopen()?));
    }
    if case != "compare" {
        return Ok(gap(case, "unknown-case", 0, 0));
    }
    let compared = drive::compare()?;
    Ok(gap(case, compared.reason, compared.records, 2))
}

fn replay_record(reopened: &replay::Reopened) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "journal-replay-v1"),
        fields: vec![
            (Symbol::new("case"), string("replay")),
            (Symbol::new("corrupt"), string(reopened.corrupt)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("head"), string(reopened.head)),
            (Symbol::new("reason"), string(reopened.reason)),
            (Symbol::new("records"), query::count_datum(reopened.records)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
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
