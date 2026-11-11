// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the cold comparison records.

use sim_kernel::Datum;

use super::{AssayBounds, Capture, ColdOthers, OwnedCompare};

const COLD: &str = "prove/cold-others-v1";
const ASSAY: &str = "world/assay-observation-v1";
const ROW: &str = "world/assay-row-v1";

pub(super) fn parsed_compare(capture: &Capture) -> Option<OwnedCompare> {
    let cold = cold_others(&capture.cold)?;
    let assay = assay_bounds(&capture.assay)?;
    if !cold.resident
        || !cold.reuse_disabled
        || cold.skipped != super::SKIP
        || cold.failures != 0
        || cold.owner_commands != 0
        || cold.requests != cold.executed
        || cold.executed.saturating_add(cold.gaps) != cold.denominator
    {
        return None;
    }
    Some(OwnedCompare {
        leaves: cold.denominator.to_string(),
        reuse: "disabled".to_owned(),
        false_reuse: assay.false_reuse.to_string(),
        d1: assay.d1.to_string(),
        d2: assay.d2.to_string(),
        d4: assay.d4.to_string(),
        assay: assay.denominator.to_string(),
    })
}

pub(super) fn effect_total(capture: &Capture) -> String {
    match (
        number(&capture.cold, "effects"),
        number(&capture.assay, "effects"),
    ) {
        (Some(cold), Some(assay)) => cold.saturating_add(assay).to_string(),
        _ => "missing".to_owned(),
    }
}

pub(super) fn subprocess_total(capture: &Capture) -> String {
    match (
        number(&capture.cold, "subprocesses"),
        number(&capture.assay, "subprocesses"),
    ) {
        (Some(cold), Some(assay)) => cold.saturating_add(assay).to_string(),
        _ => "missing".to_owned(),
    }
}

fn cold_others(value: &Datum) -> Option<ColdOthers> {
    if tag(value)? != COLD {
        return None;
    }
    Some(ColdOthers {
        denominator: number(value, "denominator")?,
        executed: number(value, "executed")?,
        requests: number(value, "requests")?,
        gaps: number(value, "gaps")?,
        failures: number(value, "failures")?,
        owner_commands: number(value, "owner-commands")?,
        reuse_disabled: flag(value, "reuse-disabled")?,
        skipped: text(value, "skipped")?,
        resident: flag(value, "resident")?,
    })
}

fn assay_bounds(value: &Datum) -> Option<AssayBounds> {
    if tag(value)? != ASSAY {
        return None;
    }
    let denominator = number(value, "denominator")?;
    let Datum::Vector(rows) = field(value, "rows")? else {
        return None;
    };
    let mut false_reuse = 0u64;
    let mut d1 = None;
    let mut d2 = None;
    let mut d4 = None;
    for row in rows {
        let bounds = row_bounds(row, denominator)?;
        false_reuse = false_reuse.saturating_add(bounds.false_reuse);
        match bounds.delta.as_str() {
            "D1" => d1 = Some(bounds.basis),
            "D2" => d2 = Some(bounds.basis),
            "D4" => d4 = Some(bounds.basis),
            _ => {}
        }
    }
    Some(AssayBounds {
        denominator,
        d1: d1?,
        d2: d2?,
        d4: d4?,
        false_reuse,
    })
}

struct RowBounds {
    delta: String,
    basis: u64,
    false_reuse: u64,
}

fn row_bounds(row: &Datum, denominator: u64) -> Option<RowBounds> {
    if tag(row)? != ROW {
        return None;
    }
    let affected = match field(row, "affected")? {
        Datum::Vector(items) => u64::try_from(items.len()).ok()?,
        _ => return None,
    };
    let basis = number(row, "unaffected-basis-points")?;
    let computed = basis_points(denominator, affected);
    let mut false_reuse = u64::from(computed != basis);
    if flag(row, "carried-state")? {
        false_reuse = false_reuse.saturating_add(1);
    }
    Some(RowBounds {
        delta: text(row, "delta")?,
        basis,
        false_reuse,
    })
}

fn basis_points(denominator: u64, affected: u64) -> u64 {
    if denominator == 0 || affected > denominator {
        return 0;
    }
    denominator.saturating_sub(affected).saturating_mul(10_000) / denominator
}

fn tag(value: &Datum) -> Option<String> {
    match value {
        Datum::Node { tag, .. } => Some(tag.as_qualified_str()),
        _ => None,
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

fn number(value: &Datum, name: &str) -> Option<u64> {
    match field(value, name)? {
        Datum::Number(number) if !number.canonical.is_empty() => number.canonical.parse().ok(),
        _ => None,
    }
}

fn flag(value: &Datum, name: &str) -> Option<bool> {
    match field(value, name)? {
        Datum::Bool(bit) => Some(*bit),
        _ => None,
    }
}

fn text(value: &Datum, name: &str) -> Option<String> {
    match field(value, name)? {
        Datum::String(text) => Some(text.clone()),
        _ => None,
    }
}
