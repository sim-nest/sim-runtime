// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::BTreeSet, time::Instant};

use sim_incremental_core::projection::{
    ConclusionId, FactId, FederatedClosure, OwnerProjectionGraph,
};
use sim_kernel::{CapabilityName, Datum, Dependency, NumberLiteral, Symbol};

use crate::WorldError;
use crate::command::render;
use crate::product::explanation_value;

const RESIDENT_RECORDS: usize = 100_000;
const RESIDENT_UNITS: usize = 10_000;
const FACTS_PER_UNIT: usize = 10;
pub(crate) const WARMUPS: usize = 20;
pub(crate) const SAMPLES: usize = 200;
pub(crate) const CEILING_MS: u64 = 200;

pub(crate) struct ResidentSnapshot {
    pub(crate) closure: FederatedClosure,
    pub(crate) records: usize,
    pub(crate) units: usize,
}

pub(crate) struct MeasuredWhy {
    pub(crate) record: Datum,
    pub(crate) within_ceiling: bool,
}

pub(crate) fn count_datum(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}

pub(crate) fn nearest_rank_p95(samples: usize) -> usize {
    (samples.saturating_mul(95).saturating_add(99) / 100).saturating_sub(1)
}

pub(crate) fn seal_resident_why() -> Result<ResidentSnapshot, WorldError> {
    let mut facts = Vec::with_capacity(RESIDENT_RECORDS);
    for index in 0..RESIDENT_RECORDS {
        facts.push(FactId::new(format!("fact/{index:06}"))?);
    }
    let mut dependencies = Vec::with_capacity(RESIDENT_UNITS);
    for (unit, window) in facts.chunks_exact(FACTS_PER_UNIT).enumerate() {
        dependencies.push((
            ConclusionId::new(format!("conclusion/unit-{unit:06}"))?,
            window.iter().cloned().collect::<BTreeSet<_>>(),
        ));
    }
    if facts.len() != RESIDENT_RECORDS || dependencies.len() != RESIDENT_UNITS {
        return Err(WorldError::Qualification(
            "resident snapshot denominator drifted".to_owned(),
        ));
    }
    let graph = OwnerProjectionGraph::new("sim-runtime/sim-lib-world", dependencies)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let closure = FederatedClosure::seal(facts, [graph])
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    Ok(ResidentSnapshot {
        closure,
        records: RESIDENT_RECORDS,
        units: RESIDENT_UNITS,
    })
}

pub(crate) fn read_only_record(
    effects: usize,
    capabilities: &[CapabilityName],
    requires: &[Dependency],
) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "read-only-v1"),
        fields: vec![
            (Symbol::new("effects"), count_datum(usize_count(effects))),
            (
                Symbol::new("capabilities"),
                Datum::Vector(
                    capabilities
                        .iter()
                        .map(|capability| Datum::String(capability.as_str().to_owned()))
                        .collect(),
                ),
            ),
            (
                Symbol::new("requires"),
                Datum::Vector(requires.iter().map(dependency_datum).collect()),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
        ],
    }
}

pub(crate) fn measure_why(effects: usize) -> Result<MeasuredWhy, WorldError> {
    let snapshot = seal_resident_why()?;
    let conclusion = ConclusionId::new("conclusion/unit-000000")?;
    let fact = FactId::new("fact/000000")?;
    let expected = render_explain(&snapshot.closure, &conclusion, &fact)?;
    for _ in 1..WARMUPS {
        let rendered = render_explain(&snapshot.closure, &conclusion, &fact)?;
        if rendered != expected {
            return Err(WorldError::Qualification(
                "resident explanation changed".to_owned(),
            ));
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let rendered = render_explain(&snapshot.closure, &conclusion, &fact)?;
        let elapsed = started.elapsed();
        if rendered != expected {
            return Err(WorldError::Qualification(
                "resident explanation changed".to_owned(),
            ));
        }
        samples.push(elapsed);
    }
    samples.sort_unstable();
    let p95_ms = u64::try_from(samples[nearest_rank_p95(SAMPLES)].as_millis()).unwrap_or(u64::MAX);
    Ok(MeasuredWhy {
        record: latency_record("why", snapshot.records, snapshot.units, p95_ms, effects),
        within_ceiling: p95_ms <= CEILING_MS,
    })
}

fn render_explain(
    closure: &FederatedClosure,
    conclusion: &ConclusionId,
    fact: &FactId,
) -> Result<String, WorldError> {
    let explanation = closure
        .explain(conclusion, fact)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    Ok(render(&explanation_value(explanation)))
}

pub(crate) fn latency_record(
    command: &str,
    records: usize,
    units: usize,
    p95_ms: u64,
    effects: usize,
) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "query-latency-v1"),
        fields: vec![
            (Symbol::new("command"), Datum::String(command.to_owned())),
            (Symbol::new("records"), count_datum(usize_count(records))),
            (Symbol::new("units"), count_datum(usize_count(units))),
            (Symbol::new("warmups"), count_datum(usize_count(WARMUPS))),
            (Symbol::new("samples"), count_datum(usize_count(SAMPLES))),
            (Symbol::new("p95-ms"), count_datum(p95_ms)),
            (Symbol::new("effects"), count_datum(usize_count(effects))),
            (Symbol::new("subprocesses"), count_datum(0)),
            (Symbol::new("resident"), Datum::Bool(true)),
        ],
    }
}

fn usize_count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn dependency_datum(dependency: &Dependency) -> Datum {
    let mut text = dependency.id.to_string();
    if let Some(version) = &dependency.minimum_version {
        text.push(' ');
        text.push_str(&version.0);
    }
    Datum::String(text)
}
