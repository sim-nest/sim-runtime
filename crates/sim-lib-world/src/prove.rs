// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure proof execution for the sealed portable subject.
//!
//! The public cold record enumerates the portable phase gates. A gate with a
//! reachable owner result is one catalog leaf. A gate without one stays a
//! named gap, and the frontier stays open. The forty conclusions remain the
//! sensitivity assay. A scripted command receipt is not a member of either
//! denominator. Opening an admitted union view reruns the grounded leaves.
//! Opening an admitted assay view reruns the sealed baseline program and the
//! retained later programs. A missing or stale view asks for a refresh and
//! does not execute. Native worker and native assay leaves stay outside both
//! denominators.

use sim_incremental_core::projection::{
    FactId, FederatedClosure, PackageIdentity, ProjectionRegistry, install_semantic_text_provider,
};
use sim_kernel::Datum;
use std::cell::Cell;
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use crate::assay;
use crate::provider::{self, WorldConfigShape};
use crate::query::{self, SAMPLES, WARMUPS};
use crate::{WorldError, command};

#[path = "prove_leaf.rs"]
mod leaf;
#[path = "prove_record.rs"]
mod record;
#[path = "prove_run.rs"]
mod run;
#[path = "portable_union.rs"]
mod union;
#[cfg(test)]
#[path = "prove_view.rs"]
mod view;

use record::{flag, hex, node, num, text};
#[cfg(test)]
pub(crate) use run::{Disturbance, digest_for_code, digest_for_delta};

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

const PAGE: usize = 8;
const DELTAS: [&str; 6] = ["D1", "D2", "D3", "D4", "D5", "D6"];
const EXCLUDED: [&str; 2] = ["leaf/native-worker-adapter", "leaf/native-assay"];

/// One leaf's reuse decision. The command follows the sealed graph. A caller
/// can keep an affected leaf or name an input the row did not read.
#[cfg(test)]
pub(crate) struct Choice {
    /// Whether this leaf runs again.
    pub(crate) rerun: bool,
    /// An extra claimed input that was not read.
    pub(crate) extra_claim: Option<String>,
}

/// Follows the sealed affected set and claims only inputs the row reads.
#[cfg(test)]
pub(crate) fn graph_choice(_leaf: &str, affected: bool) -> Choice {
    Choice {
        rerun: affected,
        extra_claim: None,
    }
}

#[cfg(test)]
#[derive(PartialEq, Eq)]
struct DeltaCold {
    changed: BTreeSet<FactId>,
    revision: u64,
    digests: BTreeMap<String, String>,
    captures: BTreeMap<String, String>,
}

/// Cold run of the sensitivity assay.
///
/// The public cold record reports the portable phase union. This value is the
/// forty-conclusion assay.
#[cfg(test)]
pub(crate) struct ColdProof {
    /// Semantic subject digest of the frozen baseline.
    pub(crate) subject: String,
    /// Selected leaf ids, in closure order.
    pub(crate) leaves: Vec<String>,
    /// Semantic-provider reads and proof reads, with reuse disabled.
    pub(crate) requests: u64,
    /// Declared dependency edges across the selected leaves.
    pub(crate) edges: u64,
    /// Leaves whose catalog predicate failed. They stay in the denominator.
    pub(crate) failures: Vec<String>,
    /// Projected text retained from each judged leaf.
    pub(crate) captures: BTreeMap<String, String>,
    /// Program text each later pass ran. The baseline keeps the sealed program.
    codes: BTreeMap<String, String>,
    /// Command leaves in the catalog that cold execution ran.
    pub(crate) owner_commands: u64,
    /// Leaf that read `hidden_fact` without declaring it.
    hidden_leaf: Option<String>,
    /// Undeclared fact read by `hidden_leaf`.
    hidden_fact: Option<String>,
    /// Fact whose raw bytes were empty for this run.
    empty_fact: Option<String>,
    baseline: BTreeMap<String, String>,
    facts: BTreeMap<String, Vec<String>>,
    deltas: BTreeMap<String, DeltaCold>,
}

/// One delta compared with the cold denominator.
#[cfg(test)]
pub(crate) struct Report {
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
}

/// Resident cached query sample.
pub(crate) struct Measured {
    /// Latency record.
    pub(crate) record: Datum,
    /// Whether nearest-rank p95 is inside the ceiling.
    pub(crate) within_ceiling: bool,
}

#[cfg(test)]
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

/// Runs every assay conclusion on the baseline and on each frozen delta.
#[cfg(test)]
pub(crate) fn execute_cold() -> Result<ColdProof, WorldError> {
    #[cfg(test)]
    COLD_EXECUTIONS.with(|cell| cell.set(cell.get().saturating_add(1)));
    run::execute(run::Disturbance::none())
}

/// Cold run whose inputs differ from the sealed subject.
///
/// Reuse compares its catalog rerun with this result. The disturbance is the
/// cold side of that comparison, not a second denominator.
#[cfg(test)]
pub(crate) fn execute_disturbed(disturbance: Disturbance) -> Result<ColdProof, WorldError> {
    COLD_EXECUTIONS.with(|cell| cell.set(cell.get().saturating_add(1)));
    run::execute(disturbance)
}

/// Refuses a command receipt that the sealed catalog does not admit.
#[cfg(test)]
pub(crate) fn admit_command(leaf: &str, scripted: bool) -> Result<(), WorldError> {
    leaf::admit_command(leaf, scripted)
}

/// Executes one catalog command on the qualified route.
///
/// The leaf is not a member of the sealed denominator.
#[cfg(test)]
pub(crate) fn run_catalog_command() -> Result<leaf::CommandRoute, WorldError> {
    leaf::run_catalog_command()
}

/// Exports one cold run as a revision-fenced view.
#[cfg(test)]
pub(crate) fn cold_view(cold: &ColdProof) -> Result<String, WorldError> {
    view::export(cold)
}

/// Compares one delta with the cold denominator.
#[cfg(test)]
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
            let digest = run::project_retained(cold, leaf, delta, &mut reuse_requests)?.digest;
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
    }
    Ok(Some(Report {
        basis: basis(cold.leaves.len(), reused.len()),
        reused,
        rerun,
        false_reuse,
        cold_requests: cold.requests,
        reuse_requests,
        revision: row.revision,
    }))
}

/// Plan page for the portable phase gates. A token opens the page it names.
pub(crate) fn plan(token: Option<&str>) -> Result<Datum, WorldError> {
    union::plan(token)
}

/// Continues a plan page or a frontier token.
pub(crate) fn resume(token: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    union::resume(token, view)
}

/// Resident proof status read from an admitted union view.
///
/// The request count is the grounded leaves reread while admitting that view.
pub(crate) fn status(view: Option<&str>) -> Result<Datum, WorldError> {
    union::status(view)
}

/// Cold phase union, or one delta's reuse comparison against an admitted view.
pub(crate) fn run(name: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    union::run(name, view)
}

/// Why one phase gate was reused, or the gap that kept it from running.
pub(crate) fn why(delta: &str, leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    union::why(delta, leaf, view)
}

/// Failure frontier of a retained phase-union run.
///
/// Opening the view reruns each grounded leaf. The request count is the
/// retained cold reads plus those admission reads. A named gap keeps the
/// frontier open.
pub(crate) fn frontier(view: Option<&str>) -> Result<Datum, WorldError> {
    union::frontier(view)
}

/// One named leaf that is outside the phase union, or the full frontier when
/// the leaf is one of the gates.
pub(crate) fn frontier_leaf(leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    union::frontier_leaf(leaf, view)
}

/// Times public `status` or `why` against an admitted phase-union view.
///
/// The cold view is admitted once, outside the samples. Each sample calls the
/// same function the prove command dispatches, then encodes the datum.
pub(crate) fn measure(command: &str) -> Result<Measured, WorldError> {
    if command != "status" && command != "why" {
        return Err(WorldError::Qualification("usage".to_owned()));
    }
    let cold = run("cold", None)?;
    let view = record_text(&cold, "view")?;
    let records = record_count(&cold, "denominator")?;
    let units = record_count(&cold, "executed")?;
    let cold_effects = record_count(&cold, "effects")?;
    let cold_subprocesses = record_count(&cold, "subprocesses")?;
    let effects = Cell::new(None);
    let subprocesses = Cell::new(None);
    let sampled = query::sample_cached(|| {
        let datum = if command == "status" {
            status(Some(&view))?
        } else {
            why("D1", "phase/07", Some(&view))?
        };
        observe_counters(&datum, &effects, &subprocesses)?;
        Ok(command::render(&datum))
    })?;
    let (Some(effects), Some(subprocesses)) = (effects.get(), subprocesses.get()) else {
        return Err(WorldError::Qualification(
            "measurement recorded no counters".to_owned(),
        ));
    };
    if effects != cold_effects || subprocesses != cold_subprocesses {
        return Err(WorldError::Qualification(
            "measured counters disagreed with the cold record".to_owned(),
        ));
    }
    Ok(Measured {
        record: node(
            "query-latency-v1",
            vec![
                ("command", text(command)),
                ("records", num(records)),
                ("units", num(units)),
                ("warmups", num(u64::try_from(WARMUPS).unwrap_or(u64::MAX))),
                ("samples", num(u64::try_from(SAMPLES).unwrap_or(u64::MAX))),
                ("p95-ms", num(sampled.p95_ms)),
                (
                    "raw-us",
                    Datum::Vector(sampled.raw_us.iter().copied().map(num).collect()),
                ),
                ("effects", num(effects)),
                ("subprocesses", num(subprocesses)),
                ("resident", flag(true)),
            ],
        ),
        within_ceiling: sampled.within_ceiling,
    })
}

fn observe_counters(
    datum: &Datum,
    effects: &Cell<Option<u64>>,
    subprocesses: &Cell<Option<u64>>,
) -> Result<(), WorldError> {
    let observed_effects = record_count(datum, "effects")?;
    let observed_subprocesses = record_count(datum, "subprocesses")?;
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

fn record_text(value: &Datum, name: &str) -> Result<String, WorldError> {
    match record_field(value, name)? {
        Datum::String(text) => Ok(text.clone()),
        _ => Err(WorldError::Qualification(format!(
            "proof record {name} is not text"
        ))),
    }
}

fn record_count(value: &Datum, name: &str) -> Result<u64, WorldError> {
    match record_field(value, name)? {
        Datum::Number(number) => number
            .canonical
            .parse()
            .map_err(|_| WorldError::Qualification(format!("proof record {name} is not a count"))),
        _ => Err(WorldError::Qualification(format!(
            "proof record {name} is not a count"
        ))),
    }
}

fn record_field<'a>(value: &'a Datum, name: &str) -> Result<&'a Datum, WorldError> {
    let Datum::Node { fields, .. } = value else {
        return Err(WorldError::Qualification(
            "proof record is not a node".to_owned(),
        ));
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, field)| field)
        .ok_or_else(|| WorldError::Qualification(format!("proof record lacks {name}")))
}

pub(super) struct BoundFacts {
    pub(super) registry: ProjectionRegistry,
    pub(super) package: PackageIdentity,
    pub(super) shape: WorldConfigShape,
    pub(super) closure: FederatedClosure,
    pub(super) facts: BTreeMap<String, Vec<FactId>>,
}

pub(super) fn bind() -> Result<BoundFacts, WorldError> {
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
    for conclusion in closure.conclusions() {
        if EXCLUDED.contains(&conclusion.as_str()) {
            return Err(WorldError::Qualification(
                "excluded leaf entered the denominator".to_owned(),
            ));
        }
    }
    let facts = crate::assay_oracle::require_union(&closure)?;
    Ok(BoundFacts {
        registry,
        package,
        shape,
        closure,
        facts,
    })
}

pub(crate) use run::attempt_leaf;

#[cfg(test)]
fn sealed_ids() -> Result<(Vec<String>, String), WorldError> {
    let bound = bind()?;
    let leaves: Vec<String> = bound.facts.keys().cloned().collect();
    if leaves.is_empty() {
        return Err(WorldError::Qualification(
            "commissioned assay is empty".to_owned(),
        ));
    }
    let subject = subject_of(&bound)?;
    Ok((leaves, subject))
}

#[cfg(test)]
fn subject_of(bound: &BoundFacts) -> Result<String, WorldError> {
    let datum = Datum::Vector(
        subject_rows(bound)?
            .into_iter()
            .map(Datum::String)
            .collect(),
    );
    Ok(hex(&provider::content_id(datum)?.bytes))
}

#[cfg(test)]
fn subject_rows(bound: &BoundFacts) -> Result<Vec<String>, WorldError> {
    let mut rows = vec![
        bound.package.name.clone(),
        bound.package.version.clone(),
        hex(&bound.package.code.bytes),
        hex(&bound.shape.id.bytes),
        leaf::proof_implementation().to_owned(),
        include_str!("prove_run.rs").to_owned(),
        include_str!("prove_view.rs").to_owned(),
        include_str!("prove_record.rs").to_owned(),
        include_str!("product.rs").to_owned(),
        include_str!("query.rs").to_owned(),
        include_str!("../../../Cargo.lock").to_owned(),
        include_str!("../../../../sim-agent-net/crates/sim-lib-roadmap-runner/src/proof.rs")
            .to_owned(),
    ];
    for (leaf, facts) in &bound.facts {
        rows.push(leaf.clone());
        rows.push(leaf::executed_code(leaf::code_for(
            facts.iter().map(|fact| fact.as_str()),
        )));
        for fact in facts {
            let name = fact.as_str();
            rows.push(name.to_owned());
            rows.push(assay::baseline_bytes(name));
            for delta in DELTAS {
                rows.push(assay::variant_bytes(delta, name));
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
pub(crate) use run::{
    delta_capture, delta_digest, open_view, proof_implementation, subject_material,
};

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

#[cfg(test)]
fn basis(denominator: usize, reused: usize) -> u16 {
    if denominator == 0 || reused > denominator {
        return 0;
    }
    u16::try_from(reused.saturating_mul(10_000) / denominator).unwrap_or(10_000)
}
