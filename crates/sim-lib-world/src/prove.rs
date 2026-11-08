// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure proof execution for the sealed portable subject.
//!
//! Cold execution runs every selected leaf with reuse disabled and exports a
//! revision-fenced view. A later reuse decision reads that view. It is kept
//! only when its digest matches the cold result and it claims no input the run
//! did not read. A missing or stale view asks for a refresh and does not
//! execute again. Native worker and native assay leaves stay outside the
//! denominator.

#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use sim_incremental_core::projection::{
    ConclusionId, FactId, FederatedClosure, PackageIdentity, ProjectionEngine, ProjectionRegistry,
    install_semantic_text_provider,
};
use sim_kernel::Datum;

use crate::assay;
use crate::product::explanation_value;
use crate::provider::{self, WorldConfigShape};
use crate::query::{self, CEILING_MS, SAMPLES, WARMUPS};
use crate::{WorldError, command};

#[path = "prove_record.rs"]
mod record;
#[path = "prove_view.rs"]
mod view;

use record::{cold_record, flag, gap, hex, list, node, num, plan_record, reuse_record, text};

#[cfg(test)]
thread_local! {
    static COLD_EXECUTIONS: Cell<usize> = const { Cell::new(0) };
}

/// Cold executions on this thread. The counter is a test probe, not a store.
#[cfg(test)]
pub(crate) fn cold_executions() -> usize {
    COLD_EXECUTIONS.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reseal_replaced(
    text: &str,
    from: &str,
    to: &str,
) -> Result<Option<String>, WorldError> {
    view::reseal_replaced(text, from, to)
}

const DENOMINATOR: usize = 40;
const PAGE: usize = 8;
const DELTAS: [&str; 6] = ["D1", "D2", "D3", "D4", "D5", "D6"];
const EXCLUDED: [&str; 2] = ["leaf/native-worker-adapter", "leaf/native-assay"];

/// One leaf's reuse decision. The command follows the sealed graph. A caller
/// can keep an affected leaf or name an input the row did not read.
pub(crate) struct Choice {
    /// Whether this leaf runs again.
    pub(crate) rerun: bool,
    /// An extra claimed input that was not read.
    pub(crate) extra_claim: Option<String>,
}

/// Follows the sealed affected set and claims only inputs the row reads.
pub(crate) fn graph_choice(_leaf: &str, affected: bool) -> Choice {
    Choice {
        rerun: affected,
        extra_claim: None,
    }
}

struct DeltaCold {
    changed: BTreeSet<FactId>,
    revision: u64,
    digests: BTreeMap<String, String>,
}

/// Complete cold denominator for the sealed subject.
pub(crate) struct ColdProof {
    /// Semantic subject digest of the frozen baseline.
    pub(crate) subject: String,
    /// Selected leaf ids, in closure order.
    pub(crate) leaves: Vec<String>,
    /// Provider reads performed with reuse disabled.
    pub(crate) requests: u64,
    /// Declared dependency edges across the selected leaves.
    pub(crate) edges: u64,
    baseline: BTreeMap<String, String>,
    facts: BTreeMap<String, Vec<String>>,
    deltas: BTreeMap<String, DeltaCold>,
}

/// One delta compared with the cold denominator.
pub(crate) struct Report {
    /// Delta name.
    pub(crate) delta: String,
    /// Leaves whose baseline digest was kept.
    pub(crate) reused: BTreeSet<String>,
    /// Leaves that ran again.
    pub(crate) rerun: BTreeSet<String>,
    /// Reused leaves over the denominator, in basis points.
    pub(crate) basis: u16,
    /// Reuse rows that disagree with the cold digests or claim an unread input.
    pub(crate) false_reuse: u64,
    /// Cold provider reads.
    pub(crate) cold_requests: u64,
    /// Provider reads on the reuse row.
    pub(crate) reuse_requests: u64,
    /// Order-only revision count carried from the sealed delta.
    pub(crate) revision: u64,
    digests: BTreeMap<String, String>,
    exercised: BTreeMap<String, Vec<String>>,
}

/// Resident cached query sample.
pub(crate) struct Measured {
    /// Latency record.
    pub(crate) record: Datum,
    /// Whether nearest-rank p95 is inside the ceiling.
    pub(crate) within_ceiling: bool,
}

impl ColdProof {
    /// Baseline digest of one selected leaf.
    #[cfg(test)]
    pub(crate) fn baseline_digest(&self, leaf: &str) -> Option<&str> {
        self.baseline.get(leaf).map(String::as_str)
    }
}

/// Fact names each selected leaf reads, in universe order.
#[cfg(test)]
pub(crate) fn leaf_facts() -> Result<BTreeMap<String, Vec<String>>, WorldError> {
    let bound = bind()?;
    Ok(bound
        .facts
        .into_iter()
        .map(|(leaf, facts)| {
            (
                leaf,
                facts
                    .into_iter()
                    .map(|fact| fact.as_str().to_owned())
                    .collect(),
            )
        })
        .collect())
}

/// Runs every selected leaf on the baseline and on each frozen delta.
pub(crate) fn execute_cold() -> Result<ColdProof, WorldError> {
    #[cfg(test)]
    COLD_EXECUTIONS.with(|cell| cell.set(cell.get().saturating_add(1)));
    let batch = assay::read_semantics()?;
    let bound = bind()?;
    let mut requests = 0u64;
    let mut baseline = BTreeMap::new();
    for (leaf, facts) in &bound.facts {
        let digest = project(&bound, facts, assay::baseline_bytes, &mut requests)?;
        baseline.insert(leaf.clone(), digest);
    }
    let mut deltas = BTreeMap::new();
    for delta in &batch.deltas {
        let mut digests = BTreeMap::new();
        for (leaf, facts) in &bound.facts {
            let digest = project(
                &bound,
                facts,
                |fact| assay::variant_bytes(delta.delta, fact),
                &mut requests,
            )?;
            digests.insert(leaf.clone(), digest);
        }
        deltas.insert(
            delta.delta.to_owned(),
            DeltaCold {
                changed: delta.changed.clone(),
                revision: delta.revision_delta,
                digests,
            },
        );
    }
    let leaves: Vec<String> = bound.facts.keys().cloned().collect();
    let edges = bound.facts.values().map(Vec::len).sum::<usize>();
    if leaves.len() != DENOMINATOR || deltas.len() != DELTAS.len() {
        return Err(WorldError::Qualification(
            "portable proof denominator drifted".to_owned(),
        ));
    }
    Ok(ColdProof {
        subject: batch.subject,
        leaves,
        requests,
        edges: u64::try_from(edges).unwrap_or(u64::MAX),
        baseline,
        facts: bound
            .facts
            .iter()
            .map(|(leaf, facts)| {
                (
                    leaf.clone(),
                    facts.iter().map(|fact| fact.as_str().to_owned()).collect(),
                )
            })
            .collect(),
        deltas,
    })
}

/// Compares one delta with the cold denominator.
pub(crate) fn reuse(
    cold: &ColdProof,
    delta: &str,
    choose: impl Fn(&str, bool) -> Choice,
) -> Result<Option<Report>, WorldError> {
    let Some(row) = cold.deltas.get(delta) else {
        return Ok(None);
    };
    let bound = bind()?;
    let affected = bound
        .closure
        .affected(row.changed.iter().cloned())
        .into_iter()
        .map(|conclusion| conclusion.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut reused = BTreeSet::new();
    let mut rerun = BTreeSet::new();
    let mut digests = BTreeMap::new();
    let mut exercised = BTreeMap::new();
    let mut false_reuse = 0u64;
    let mut reuse_requests = 0u64;
    for leaf in &cold.leaves {
        let Some(facts) = cold.facts.get(leaf) else {
            return Err(WorldError::Qualification(format!(
                "unavailable-proof {leaf}"
            )));
        };
        let cold_digest = row
            .digests
            .get(leaf)
            .ok_or_else(|| WorldError::Qualification(format!("unavailable-proof {leaf}")))?;
        let baseline = cold
            .baseline
            .get(leaf)
            .ok_or_else(|| WorldError::Qualification(format!("unavailable-proof {leaf}")))?;
        let choice = choose(leaf, affected.contains(leaf));
        let run_again = choice.rerun;
        let (digest, read) = if run_again {
            let fact_ids = facts
                .iter()
                .map(|fact| FactId::new(fact.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            let digest = project(
                &bound,
                &fact_ids,
                |fact| assay::variant_bytes(delta, fact),
                &mut reuse_requests,
            )?;
            (digest, facts.clone())
        } else {
            (baseline.clone(), Vec::new())
        };
        let actual: BTreeSet<String> = read.iter().cloned().collect();
        let mut claimed = actual.clone();
        if let Some(fact) = choice.extra_claim {
            claimed.insert(fact);
        }
        if &digest != cold_digest || claimed != actual {
            false_reuse = false_reuse.saturating_add(1);
        }
        if run_again {
            rerun.insert(leaf.clone());
        } else {
            reused.insert(leaf.clone());
        }
        digests.insert(leaf.clone(), digest);
        exercised.insert(leaf.clone(), read);
    }
    Ok(Some(Report {
        delta: delta.to_owned(),
        basis: basis(cold.leaves.len(), reused.len()),
        reused,
        rerun,
        false_reuse,
        cold_requests: cold.requests,
        reuse_requests,
        revision: row.revision,
        digests,
        exercised,
    }))
}

/// Plan page for the selected leaves. A token opens the page it names.
pub(crate) fn plan(token: Option<&str>) -> Result<Datum, WorldError> {
    let (leaves, subject) = sealed_ids()?;
    let pages = page_count(leaves.len());
    let page = match token {
        None => 0,
        Some(token) => match page_for(token, &subject, pages)? {
            Some(page) => page,
            None => return Ok(gap("plan", "stale-or-missing")),
        },
    };
    plan_record(&leaves, &subject, page, pages)
}

/// Continues a plan page or a completed frontier token.
pub(crate) fn resume(token: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if !is_token(token) {
        return Ok(gap("resume", "stale-or-missing"));
    }
    let (leaves, subject) = sealed_ids()?;
    let pages = page_count(leaves.len());
    if let Some(page) = page_for(token, &subject, pages)? {
        return plan_record(&leaves, &subject, page, pages);
    }
    if token == frontier_token(&subject)? {
        return frontier(view);
    }
    Ok(gap("resume", "stale-or-missing"))
}

/// Resident proof status read from an admitted view. The query dispatches nothing.
pub(crate) fn status(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted(view)? else {
        return Ok(gap("status", "refresh-needed"));
    };
    Ok(node(
        "status-v1",
        vec![
            (
                "denominator",
                num(u64::try_from(opened.leaves.len()).unwrap_or(u64::MAX)),
            ),
            ("excluded", list(EXCLUDED)),
            ("requests", num(0)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("owner-commands", num(0)),
            ("resident", flag(true)),
        ],
    ))
}

/// Cold run, or one delta's reuse comparison against an admitted view.
pub(crate) fn run(name: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if name == "cold" {
        return cold_record(&execute_cold()?);
    }
    if !DELTAS.contains(&name) {
        return Ok(gap(name, "unknown-delta"));
    }
    let Some(cold) = admitted(view)? else {
        return Ok(gap(name, "refresh-needed"));
    };
    let Some(report) = reuse(&cold, name, graph_choice)? else {
        return Ok(gap(name, "unknown-delta"));
    };
    Ok(reuse_record(&report))
}

/// Why one leaf was reused or run again, read from an admitted view.
pub(crate) fn why(delta: &str, leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    match admission(leaf)? {
        Admission::Excluded => return Ok(gap(leaf, "excluded-scope")),
        Admission::Unavailable => return Ok(gap(leaf, "unavailable-proof")),
        Admission::Selected => {}
    }
    if !DELTAS.contains(&delta) {
        return Ok(gap(delta, "unknown-delta"));
    }
    let Some(cold) = admitted(view)? else {
        return Ok(gap(delta, "refresh-needed"));
    };
    let Some(report) = reuse(&cold, delta, graph_choice)? else {
        return Ok(gap(delta, "unknown-delta"));
    };
    let Some(digest) = report.digests.get(leaf) else {
        return Ok(gap(leaf, "unavailable-proof"));
    };
    let exercised = report.exercised.get(leaf).cloned().unwrap_or_default();
    Ok(node(
        "why-v1",
        vec![
            ("delta", text(delta)),
            ("leaf", text(leaf)),
            ("reused", flag(report.reused.contains(leaf))),
            ("exercised", list(exercised)),
            ("digest", text(digest)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
        ],
    ))
}

/// Failure frontier of a retained cold run. It does not execute that run again.
pub(crate) fn frontier(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted_open(view)? else {
        return Ok(gap("frontier", "refresh-needed"));
    };
    Ok(node(
        "frontier-v1",
        vec![
            ("complete", flag(true)),
            ("failures", Datum::Vector(Vec::new())),
            (
                "executed",
                num(u64::try_from(opened.proof.leaves.len()).unwrap_or(u64::MAX)),
            ),
            ("requests", num(opened.proof.requests)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("continuation", text(frontier_token(&opened.subject)?)),
        ],
    ))
}

/// One named leaf that is outside the selected set, or the full frontier when
/// the leaf is selected.
pub(crate) fn frontier_leaf(leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    match admission(leaf)? {
        Admission::Excluded => Ok(gap(leaf, "excluded-scope")),
        Admission::Unavailable => Ok(gap(leaf, "unavailable-proof")),
        Admission::Selected => frontier(view),
    }
}

fn admitted(view: Option<&str>) -> Result<Option<ColdProof>, WorldError> {
    Ok(admitted_open(view)?.map(|opened| opened.proof))
}

fn admitted_open(view: Option<&str>) -> Result<Option<view::Opened>, WorldError> {
    let Some(view) = view else {
        return Ok(None);
    };
    view::open(view)
}

/// Times resident status or why against the cached-query ceiling.
pub(crate) fn measure(command: &str) -> Result<Measured, WorldError> {
    if command != "status" && command != "why" {
        return Err(WorldError::Qualification("usage".to_owned()));
    }
    let snapshot = query::seal_resident_why()?;
    let conclusion = ConclusionId::new("conclusion/unit-000000")?;
    let fact = FactId::new("fact/000000")?;
    let once = |command: &str| -> Result<String, WorldError> {
        if command == "status" {
            if !snapshot.closure.depends_on(&conclusion, &fact) {
                return Err(WorldError::Qualification(
                    "resident status leaf is missing".to_owned(),
                ));
            }
            return Ok(format!("status {}", conclusion.as_str()));
        }
        let explanation = snapshot
            .closure
            .explain(&conclusion, &fact)
            .map_err(|error| WorldError::Qualification(error.to_string()))?;
        Ok(command::render(&explanation_value(explanation)))
    };
    let expected = once(command)?;
    for _ in 1..WARMUPS {
        if once(command)? != expected {
            return Err(WorldError::Qualification(
                "resident proof answer changed".to_owned(),
            ));
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let rendered = once(command)?;
        let elapsed = started.elapsed();
        if rendered != expected {
            return Err(WorldError::Qualification(
                "resident proof answer changed".to_owned(),
            ));
        }
        samples.push(elapsed);
    }
    samples.sort_unstable();
    let p95_ms =
        u64::try_from(samples[query::nearest_rank_p95(SAMPLES)].as_millis()).unwrap_or(u64::MAX);
    Ok(Measured {
        record: node(
            "query-latency-v1",
            vec![
                ("command", text(command)),
                (
                    "records",
                    num(u64::try_from(snapshot.records).unwrap_or(u64::MAX)),
                ),
                (
                    "units",
                    num(u64::try_from(snapshot.units).unwrap_or(u64::MAX)),
                ),
                ("warmups", num(u64::try_from(WARMUPS).unwrap_or(u64::MAX))),
                ("samples", num(u64::try_from(SAMPLES).unwrap_or(u64::MAX))),
                ("p95-ms", num(p95_ms)),
                ("effects", num(0)),
                ("subprocesses", num(0)),
                ("resident", flag(true)),
            ],
        ),
        within_ceiling: p95_ms <= CEILING_MS,
    })
}

struct BoundFacts {
    registry: ProjectionRegistry,
    package: PackageIdentity,
    shape: WorldConfigShape,
    closure: FederatedClosure,
    facts: BTreeMap<String, Vec<FactId>>,
}

fn bind() -> Result<BoundFacts, WorldError> {
    let shape_id = provider::content_id(Datum::String("shape/world-config-v1".to_owned()))?;
    let requested = PackageIdentity {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        code: provider::content_id(Datum::String("world/semantic-text-caller".to_owned()))?,
    };
    let mut registry = ProjectionRegistry::new();
    let package = install_semantic_text_provider(&mut registry, shape_id.clone(), requested)?;
    let shape = WorldConfigShape { id: shape_id };
    let closure = assay::seal_subject(None)?;
    let universe = assay::universe()?;
    let mut facts = BTreeMap::new();
    for conclusion in closure.conclusions() {
        let id = conclusion.as_str();
        if EXCLUDED.contains(&id) {
            return Err(WorldError::Qualification(
                "excluded leaf entered the denominator".to_owned(),
            ));
        }
        let required = universe
            .iter()
            .filter(|fact| closure.depends_on(conclusion, fact))
            .cloned()
            .collect::<Vec<_>>();
        if required.is_empty() {
            return Err(WorldError::Qualification(format!("unavailable-proof {id}")));
        }
        facts.insert(id.to_owned(), required);
    }
    Ok(BoundFacts {
        registry,
        package,
        shape,
        closure,
        facts,
    })
}

fn project(
    bound: &BoundFacts,
    facts: &[FactId],
    raw: impl Fn(&str) -> String,
    requests: &mut u64,
) -> Result<String, WorldError> {
    let engine = ProjectionEngine::new(&bound.registry, &bound.shape, &bound.closure);
    let mut projected = Vec::with_capacity(facts.len());
    for fact in facts {
        projected.push(assay::project_fact(
            &engine,
            &bound.package,
            &bound.shape,
            fact,
            &raw(fact.as_str()),
            requests,
        )?);
    }
    let id = provider::content_id(Datum::Vector(projected))?;
    Ok(hex(&id.bytes))
}

enum Admission {
    Selected,
    Excluded,
    Unavailable,
}

fn admission(leaf: &str) -> Result<Admission, WorldError> {
    if EXCLUDED.contains(&leaf) {
        return Ok(Admission::Excluded);
    }
    let (leaves, _) = sealed_ids()?;
    if leaves.iter().any(|id| id == leaf) {
        Ok(Admission::Selected)
    } else {
        Ok(Admission::Unavailable)
    }
}

fn sealed_ids() -> Result<(Vec<String>, String), WorldError> {
    let bound = bind()?;
    let leaves: Vec<String> = bound.facts.keys().cloned().collect();
    if leaves.len() != DENOMINATOR {
        return Err(WorldError::Qualification(
            "portable proof denominator drifted".to_owned(),
        ));
    }
    let subject = subject_of(&bound)?;
    Ok((leaves, subject))
}

fn subject_of(bound: &BoundFacts) -> Result<String, WorldError> {
    let mut rows = vec![
        bound.package.name.clone(),
        bound.package.version.clone(),
        hex(&bound.package.code.bytes),
        hex(&bound.shape.id.bytes),
    ];
    for (leaf, facts) in &bound.facts {
        rows.push(leaf.clone());
        for fact in facts {
            let name = fact.as_str();
            rows.push(name.to_owned());
            rows.push(assay::baseline_bytes(name));
            for delta in DELTAS {
                rows.push(assay::variant_bytes(delta, name));
            }
        }
    }
    let datum = Datum::Vector(rows.into_iter().map(Datum::String).collect());
    Ok(hex(&provider::content_id(datum)?.bytes))
}

fn plan_token(subject: &str, page: usize) -> Result<String, WorldError> {
    token(&format!("prove/plan/{subject}/{page}"))
}

fn frontier_token(subject: &str) -> Result<String, WorldError> {
    token(&format!("prove/frontier/{subject}"))
}

fn token(body: &str) -> Result<String, WorldError> {
    Ok(hex(
        &provider::content_id(Datum::String(body.to_owned()))?.bytes
    ))
}

fn page_count(leaves: usize) -> usize {
    leaves.div_ceil(PAGE)
}

fn page_for(token: &str, subject: &str, pages: usize) -> Result<Option<usize>, WorldError> {
    if !is_token(token) {
        return Ok(None);
    }
    for page in 1..pages {
        if plan_token(subject, page)? == token {
            return Ok(Some(page));
        }
    }
    Ok(None)
}

fn is_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn basis(denominator: usize, reused: usize) -> u16 {
    if denominator == 0 || reused > denominator {
        return 0;
    }
    u16::try_from(reused.saturating_mul(10_000) / denominator).unwrap_or(10_000)
}
