// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded promote command.
//!
//! Each case admits one registry publication set. A later identical case
//! returns that record. A private value is refused before admission.
//! The command does not contact a registry.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::query;

#[path = "promote_admit.rs"]
pub(crate) mod law;

pub(crate) use law::REGISTRY;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Admits one named registry publication set.
///
/// # Errors
/// Returns an error when the publication record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if leaked(case) {
        return Ok(gap("", "private-leakage", ""));
    }
    let Some(set) = law::table(case) else {
        return Ok(gap(case, "unknown-case", ""));
    };
    match law::admit(&set) {
        Ok(census) => Ok(success(case, &census)),
        Err(fault) => Ok(gap(case, fault.code(), "")),
    }
}

fn success(case: &str, census: &law::Census) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "promote-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("needs"), query::count_datum(census.needs)),
            (Symbol::new("packages"), query::count_datum(census.packages)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn gap(case: &str, reason: &str, evidence: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "promote-gap-v1"),
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
