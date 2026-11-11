// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded binding command.
//!
//! Each case runs the published pack binding once. A later or concurrent
//! identical case returns that record. One binding serves two subjects and
//! two scopes. Swapped identities refuse. A missing pack scope is
//! unimplemented, and an unknown scope refuses. The facet law passes. A
//! private value is refused before the reservation.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "bind_drive.rs"]
mod drive;
#[path = "bind_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Runs one named binding case through the published pack law.
///
/// # Errors
/// Returns an error when the binding record cannot be represented.
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
    matches!(case, "serve" | "swapped" | "missing" | "facet")
        || cfg!(test) && case == "bind-memory-race"
}

/// Maps the reservation-race case onto the shared binding.
///
/// The race key is not a second binding law. Tests use it so two calls of
/// the loaded command share one binding record.
fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "bind-memory-race" {
        return "serve";
    }
    case
}

fn receive(case: &str) -> Datum {
    let law = match case {
        "serve" => drive::serve(),
        "swapped" => drive::swapped(),
        "missing" => drive::missing(),
        "facet" => drive::facet(),
        _ => return gap(case, "unknown-case", ""),
    };
    match law {
        Ok(law) => success(case, &law),
        Err(fault) => gap(case, fault.code(), ""),
    }
}

fn success(case: &str, law: &drive::Law) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "bind-v1"),
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
        tag: Symbol::qualified("world", "bind-gap-v1"),
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
