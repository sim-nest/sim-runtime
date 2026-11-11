// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded conduct command.
//!
//! Each case drives the published one-effect conductor once. A later or
//! concurrent identical case returns that record. One drive admits at most
//! one effect or one pure transition. A no-change wake starts nothing. A
//! wait resumes once on a material offer. A stale, wrong, replayed, or
//! advisory direction refuses. Inspection reads the retained result and
//! does not conduct. A private value is refused before the reservation.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "conduct_drive.rs"]
mod drive;
#[path = "conduct_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Runs one named conduct case through the published conductor.
///
/// # Errors
/// Returns an error when the conductor record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if leaked(case) {
        return Ok(gap("", "private-leakage", ""));
    }
    if !known(case) {
        return Ok(gap(case, "unknown-case", ""));
    }
    let claim = match memory::claim(case) {
        memory::Reservation::Ready(found) => return Ok(found),
        memory::Reservation::Mine(claim) => claim,
    };
    let rendered = receive(canonical_case(case));
    claim.commit(&rendered);
    Ok(rendered)
}

fn known(case: &str) -> bool {
    matches!(case, "quiet" | "one" | "offer" | "direction")
        || cfg!(test) && case == "conduct-memory-race"
}

/// Maps the reservation-race case onto the quiescent drive.
///
/// The race key is not a second conduct law. Tests use it so two calls of
/// the loaded command share one conductor record.
fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "conduct-memory-race" {
        return "quiet";
    }
    case
}

fn receive(case: &str) -> Datum {
    let law = match case {
        "quiet" => drive::quiet(),
        "one" => drive::one(),
        "offer" => drive::offer(),
        "direction" => drive::direction(),
        _ => return gap(case, "unknown-case", ""),
    };
    match law {
        Ok(law) => success(case, &law),
        Err(error) => gap(case, "service", &drive::service_token(&error)),
    }
}

fn success(case: &str, law: &drive::Law) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "conduct-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("detail"), string(shown(&law.detail))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("invokes"), query::count_datum(law.invokes)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn gap(case: &str, reason: &str, evidence: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "conduct-gap-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("evidence"), string(shown(evidence))),
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
