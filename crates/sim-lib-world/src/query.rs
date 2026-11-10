// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cell::Cell, collections::BTreeSet, time::Instant};

use sim_incremental_core::projection::{
    ConclusionId, FactId, FederatedClosure, OwnerProjectionGraph,
};
use sim_kernel::{CapabilityName, Datum, Dependency, NumberLiteral, Symbol};

use crate::WorldError;
use crate::command::render;
use crate::product::WorldProduct;

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

pub(crate) fn measure_why() -> Result<MeasuredWhy, WorldError> {
    let snapshot = seal_resident_why()?;
    let records = snapshot.records;
    let units = snapshot.units;
    let product = WorldProduct::resident(snapshot.closure)?;
    let effects = Cell::new(None);
    let subprocesses = Cell::new(None);
    let sampled = sample_cached(|| {
        let (datum, work) = product.why_work("conclusion/unit-000000", "fact/000000")?;
        work.record(&effects, &subprocesses)?;
        Ok(render(&datum))
    })?;
    let (effects, subprocesses) = observed_pair(&effects, &subprocesses)?;
    Ok(MeasuredWhy {
        record: latency_record(
            "why",
            records,
            units,
            sampled.p95_ms,
            effects,
            subprocesses,
            &sampled.raw_us,
        ),
        within_ceiling: sampled.within_ceiling,
    })
}

/// Host calls one command invocation recorded while it ran.
///
/// Effect ids and subprocess identities are calls that invocation made.
/// `compiler_opened` records whether that same invocation opened a compiler.
#[derive(Clone, Debug, Default)]
pub(crate) struct HostWork {
    effects: Vec<String>,
    subprocesses: Vec<String>,
    reads: u64,
    compiler_opened: bool,
}

impl HostWork {
    /// Counts one executor receipt. Absent ids are calls the receipt did not make.
    pub(crate) fn observe(effect_id: Option<&str>, launcher: Option<&str>) -> Self {
        let mut work = Self::default();
        if let Some(id) = effect_id {
            work.effects.push(id.to_owned());
        }
        if let Some(id) = launcher {
            work.subprocesses.push(id.to_owned());
        }
        work
    }

    pub(crate) fn effects(&self) -> u64 {
        u64::try_from(self.effects.len()).unwrap_or(u64::MAX)
    }

    pub(crate) fn subprocesses(&self) -> u64 {
        u64::try_from(self.subprocesses.len()).unwrap_or(u64::MAX)
    }

    pub(crate) fn compiler_opened(&self) -> bool {
        self.compiler_opened
    }

    /// Records one dependency or member this command read.
    ///
    /// A read is not an effect invocation and does not launch a subprocess.
    pub(crate) fn read(&mut self) {
        self.reads = self.reads.saturating_add(1);
    }

    pub(crate) fn reads(&self) -> u64 {
        self.reads
    }

    pub(crate) fn record(
        &self,
        effects: &Cell<Option<u64>>,
        subprocesses: &Cell<Option<u64>>,
    ) -> Result<(), WorldError> {
        let _reads = self.reads();
        keep(effects, subprocesses, self.effects(), self.subprocesses())
    }
}

pub(crate) fn observed_pair(
    effects: &Cell<Option<u64>>,
    subprocesses: &Cell<Option<u64>>,
) -> Result<(usize, usize), WorldError> {
    match (effects.get(), subprocesses.get()) {
        (Some(effects), Some(subprocesses)) => Ok((
            usize::try_from(effects).unwrap_or(usize::MAX),
            usize::try_from(subprocesses).unwrap_or(usize::MAX),
        )),
        _ => Err(WorldError::Qualification(
            "measurement recorded no counters".to_owned(),
        )),
    }
}

fn keep(
    effects: &Cell<Option<u64>>,
    subprocesses: &Cell<Option<u64>>,
    observed_effects: u64,
    observed_subprocesses: u64,
) -> Result<(), WorldError> {
    match (effects.get(), subprocesses.get()) {
        (None, None) => {
            effects.set(Some(observed_effects));
            subprocesses.set(Some(observed_subprocesses));
            Ok(())
        }
        (Some(kept_effects), Some(kept_subprocesses))
            if kept_effects == observed_effects && kept_subprocesses == observed_subprocesses =>
        {
            Ok(())
        }
        _ => Err(WorldError::Qualification(
            "measured counters changed during sampling".to_owned(),
        )),
    }
}

/// Times `once` through complete output encoding.
///
/// The first `WARMUPS` calls are untimed. The next `SAMPLES` calls are the
/// nearest-rank measurement. `once` is the public command plus its encoding.
pub(crate) fn sample_cached(
    mut once: impl FnMut() -> Result<String, WorldError>,
) -> Result<Sampled, WorldError> {
    let expected = once()?;
    for _ in 1..WARMUPS {
        if once()? != expected {
            return Err(WorldError::Qualification(
                "cached answer changed during warmup".to_owned(),
            ));
        }
    }
    let mut raw_us = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let rendered = once()?;
        let elapsed = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        if rendered != expected {
            return Err(WorldError::Qualification(
                "cached answer changed during sampling".to_owned(),
            ));
        }
        raw_us.push(elapsed);
    }
    let mut ordered = raw_us.clone();
    ordered.sort_unstable();
    let p95_us = ordered[nearest_rank_p95(SAMPLES)];
    let p95_ms = p95_us / 1_000;
    Ok(Sampled {
        p95_ms,
        raw_us,
        within_ceiling: p95_ms <= CEILING_MS,
    })
}

pub(crate) struct Sampled {
    /// Nearest-rank p95 in whole milliseconds.
    pub(crate) p95_ms: u64,
    /// Sample durations in microseconds, in call order.
    pub(crate) raw_us: Vec<u64>,
    /// Whether `p95_ms` is within the cached-query ceiling.
    pub(crate) within_ceiling: bool,
}

pub(crate) fn latency_record(
    command: &str,
    records: usize,
    units: usize,
    p95_ms: u64,
    effects: usize,
    subprocesses: usize,
    raw_us: &[u64],
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
            (
                Symbol::new("raw-us"),
                Datum::Vector(raw_us.iter().copied().map(count_datum).collect()),
            ),
            (Symbol::new("effects"), count_datum(usize_count(effects))),
            (
                Symbol::new("subprocesses"),
                count_datum(usize_count(subprocesses)),
            ),
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

#[cfg(test)]
mod tests {
    use super::HostWork;

    #[test]
    fn recorded_host_calls_are_counted() {
        let quiet = HostWork::observe(None, None);
        assert_eq!(quiet.effects(), 0);
        assert_eq!(quiet.subprocesses(), 0);
        assert!(!quiet.compiler_opened());
        let work = HostWork::observe(Some("effect/read"), Some("launcher/read"));
        assert_eq!(work.effects(), 1);
        assert_eq!(work.subprocesses(), 1);
        assert!(!work.compiler_opened());
        let mut walked = HostWork::default();
        walked.read();
        assert_eq!(walked.effects(), 0);
        assert_eq!(walked.subprocesses(), 0);
        assert!(!walked.compiler_opened());
    }
}
