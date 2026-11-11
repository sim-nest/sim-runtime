// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsers for work command records.

use sim_kernel::Datum;

use super::{OwnedGap, OwnedWork};

pub(super) fn parsed_work(value: &Datum) -> Option<OwnedWork> {
    if super::kind_of(value) != super::RECORD {
        return None;
    }
    Some(OwnedWork {
        case: text(value, "case")?,
        class: text(value, "class")?,
        bytes: number(value, "bytes")?,
        files: number(value, "files")?,
        tokens: number(value, "tokens")?,
        path: text(value, "path")?,
        role: text(value, "role")?,
        status: text(value, "status")?,
        receiver: text(value, "receiver")?,
        packet: text(value, "packet")?,
        proposal: text(value, "proposal")?,
        partial: text(value, "partial")?,
        attempts: number(value, "attempts")?,
        reads: number(value, "reads")?,
        workers: number(value, "worker-calls")?,
        proposals: number(value, "proposal-calls")?,
    })
}

pub(super) fn parsed_gap(value: &Datum) -> Option<OwnedGap> {
    if super::kind_of(value) != super::GAP {
        return None;
    }
    Some(OwnedGap {
        case: text(value, "case")?,
        reason: text(value, "reason")?,
        reads: number(value, "reads")?,
        attempts: number(value, "attempts")?,
        evidence: text(value, "evidence")?,
        mirrors: text(value, "mirrors")?,
        workers: number(value, "worker-calls")?,
        proposals: number(value, "proposal-calls")?,
    })
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match super::field(value, name) {
        Some(Datum::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn number(value: &Datum, name: &str) -> Option<String> {
    match super::field(value, name) {
        Some(Datum::Number(number)) if !number.canonical.is_empty() => {
            Some(number.canonical.clone())
        }
        _ => None,
    }
}
