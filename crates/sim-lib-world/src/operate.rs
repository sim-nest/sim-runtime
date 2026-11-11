// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded operation command.
//!
//! One measure runs the operation lifecycle and the durable operation
//! service. A verified dispatch is replayed without a second performance.
//! A missing acknowledgement stays dispatched. A forged transition is
//! refused. An interruption before or after an admission recovers without
//! repeating the dispatch. A later or concurrent identical case returns
//! that record. A private value is refused before the reservation.

use std::cell::Cell;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "operate_cut.rs"]
mod cut;
#[path = "operate_drive.rs"]
mod drive;
#[path = "operate_memory.rs"]
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

/// Runs one named operation case through the operation owner.
///
/// # Errors
/// Returns an error when the operation record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    note_call();
    if leaked(case) {
        return Ok(gap("", "private-leakage"));
    }
    if !known(case) {
        return Ok(gap(case, "unknown-case"));
    }
    let claim = match memory::claim(case) {
        memory::Reservation::Ready(found) => return Ok(found),
        memory::Reservation::Mine(claim) => claim,
    };
    let rendered = receive(canonical_case(case));
    claim.commit(&rendered);
    Ok(rendered)
}

/// Operation-command entries on this thread, including a retained record.
#[cfg(test)]
pub(crate) fn calls_on_this_thread() -> u64 {
    CALLS.with(Cell::get)
}

fn note_call() {
    CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

fn known(case: &str) -> bool {
    matches!(case, "measure") || cfg!(test) && case == "operate-memory-race"
}

/// Maps the reservation-race case onto the shared measurement.
///
/// The race key is not a second operation law. Tests use it so two calls of
/// the loaded command share one operation record.
fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "operate-memory-race" {
        return "measure";
    }
    case
}

fn receive(case: &str) -> Datum {
    if case != "measure" {
        return gap(case, "unknown-case");
    }
    match drive::machine() {
        Ok(measured) => success(case, &measured),
        Err(WorldError::Qualification(reason)) => gap(case, &reason),
        Err(WorldError::Projection(_)) | Err(WorldError::UnsupportedPair { .. }) => {
            gap(case, "operation-failed")
        }
    }
}

fn success(case: &str, measured: &drive::Measured) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "operate-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("counter"), string(measured.counter)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("forged"), string(measured.forged)),
            (
                Symbol::new("interrupts"),
                query::count_datum(measured.interrupts),
            ),
            (Symbol::new("invokes"), query::count_datum(measured.invokes)),
            (
                Symbol::new("recovered"),
                query::count_datum(measured.recovered),
            ),
            (Symbol::new("replay"), string(measured.replay)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("uncertain"), string(measured.uncertain)),
            (Symbol::new("verified"), string(measured.verified)),
        ],
    }
}

fn gap(case: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "operate-gap-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("evidence"), string("")),
            (Symbol::new("reason"), string(shown(reason))),
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
