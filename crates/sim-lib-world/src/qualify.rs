// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded qualify command.
//!
//! Each case runs the published composition specimen once. A later or
//! concurrent identical case returns that record. The specimen keeps one
//! same-revision page, the exact lineage, a begin that is separate from
//! selection, and rejected advice. A Linux provider and a changed closure
//! refuse. Reading the retained result does not qualify again. A private
//! value is refused before the reservation.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "qualify_drive.rs"]
mod drive;
#[path = "qualify_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Runs one named qualify case through the published specimen.
///
/// # Errors
/// Returns an error when the specimen record cannot be represented.
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
    matches!(
        case,
        "composition" | "linux" | "freeze" | "advice" | "begin"
    ) || cfg!(test) && case == "qualify-memory-race"
}

/// Maps the reservation-race case onto the Linux refusal.
///
/// The race key is not a second composition law. Tests use it so two calls
/// of the loaded command share one specimen record.
fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "qualify-memory-race" {
        return "linux";
    }
    case
}

fn receive(case: &str) -> Datum {
    let law = match case {
        "composition" => drive::composition(),
        "linux" => drive::linux(),
        "freeze" => drive::freeze(),
        "advice" => drive::advice(),
        "begin" => drive::begin(),
        _ => return gap(case, "unknown-case", ""),
    };
    match law {
        Ok(law) => success(case, &law),
        Err(fault) => gap(case, fault.code(), ""),
    }
}

fn success(case: &str, law: &drive::Law) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "qualify-v1"),
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
        tag: Symbol::qualified("world", "qualify-gap-v1"),
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
