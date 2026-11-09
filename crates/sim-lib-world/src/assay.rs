// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::{
    ConclusionId, DeclaredInputSelector, DeterministicImportManifest, ExecutionSemantics, FactId,
    FederatedClosure, ObservedFact, ObservedWorld, OwnerProjectionGraph, PackageIdentity,
    ProjectionBudget, ProjectionEngine, ProjectionKindRef, ProjectionRegistry, ProjectionSpec,
    ProjectorPolicy, SEMANTIC_TEXT_KIND, install_semantic_text_provider,
};
use sim_kernel::{Datum, Symbol};

use crate::provider::{self, WorldConfigShape};
use crate::query::count_datum;
use crate::{WorldError, product::explanation_value};

const DELTAS: [&str; 6] = ["D1", "D2", "D3", "D4", "D5", "D6"];
const CHANGE_FACTS: [&str; 8] = [
    "change/private-implementation",
    "change/public-api",
    "change/head-a",
    "change/head-b",
    "change/head-c",
    "change/control-pins",
    "change/proof-code-alpha",
    "change/checker-alpha",
];

/// One graph edit used to show that a declaration or edge mutant misses the oracle.
#[derive(Clone, Copy)]
pub(crate) enum GraphMutation {
    /// Drop a scenario conclusion, or add an undeclared private read.
    Declaration(usize),
    /// Replace one declared edge.
    Edge(usize, usize),
}

pub(crate) struct SemanticDelta {
    pub(crate) delta: &'static str,
    pub(crate) changed: BTreeSet<FactId>,
    pub(crate) revision_delta: u64,
}

pub(crate) struct SemanticBatch {
    pub(crate) deltas: Vec<SemanticDelta>,
    pub(crate) requests: u64,
    pub(crate) effects: u64,
    pub(crate) subprocesses: u64,
    pub(crate) subject: String,
    pub(crate) facts: usize,
}

pub(crate) struct AssayRow {
    pub(crate) delta: &'static str,
    pub(crate) changed: BTreeSet<FactId>,
    pub(crate) affected: BTreeSet<ConclusionId>,
    pub(crate) unaffected_basis_points: u16,
    pub(crate) revision_delta: u64,
    pub(crate) carried_state: bool,
    pub(crate) explanations: Vec<Datum>,
    pub(crate) planned_starts: usize,
    pub(crate) journal_records: usize,
}

pub(crate) struct AssayObservation {
    pub(crate) rows: Vec<AssayRow>,
    pub(crate) requests: u64,
    pub(crate) effects: u64,
    pub(crate) subprocesses: u64,
    pub(crate) subject: String,
    pub(crate) denominator: usize,
    pub(crate) facts: usize,
}

pub(crate) fn observe() -> Result<AssayObservation, WorldError> {
    let batch = read_semantics()?;
    let covered = u64::try_from(batch.facts)
        .unwrap_or(u64::MAX)
        .saturating_mul(12);
    if batch.facts == 0 || batch.requests != covered {
        return Err(WorldError::Qualification(
            "provider reads did not cover the frozen subject".to_owned(),
        ));
    }
    let sound = judge(&batch, None)?;
    if !crate::assay_oracle::matches(&sound) {
        return Err(WorldError::Qualification(
            "prediction missed the independent oracle".to_owned(),
        ));
    }
    crate::assay_mutant::ensure_mutants_diverge(&batch)?;
    Ok(sound)
}

pub(crate) fn read_semantics() -> Result<SemanticBatch, WorldError> {
    let facts = universe()?;
    let shape_id = provider::content_id(Datum::String("shape/world-config-v1".to_owned()))?;
    let requested = PackageIdentity {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        code: provider::content_id(Datum::String("world/semantic-text-caller".to_owned()))?,
    };
    let mut registry = ProjectionRegistry::new();
    let package = install_semantic_text_provider(&mut registry, shape_id.clone(), requested)?;
    let shape = WorldConfigShape { id: shape_id };
    let closure = seal_subject(None)?;
    let engine = ProjectionEngine::new(&registry, &shape, &closure);
    let mut requests = 0u64;
    let mut effects = 0u64;
    let mut subprocesses = 0u64;
    let mut deltas = Vec::with_capacity(DELTAS.len());
    for delta in DELTAS {
        let mut changed = BTreeSet::new();
        let mut order_revisions = 0u64;
        for fact in &facts {
            let before = baseline_bytes(fact.as_str());
            let after = variant_bytes(delta, fact.as_str());
            let before_value =
                project_fact(&engine, &package, &shape, fact, &before, &mut requests)?;
            let after_value = project_fact(&engine, &package, &shape, fact, &after, &mut requests)?;
            let (before_effects, before_processes) = projection_boundary(&before_value);
            let (after_effects, after_processes) = projection_boundary(&after_value);
            effects = effects
                .saturating_add(before_effects)
                .saturating_add(after_effects);
            subprocesses = subprocesses
                .saturating_add(before_processes)
                .saturating_add(after_processes);
            if before_value.content_id().map_err(canonical)?
                != after_value.content_id().map_err(canonical)?
            {
                changed.insert(fact.clone());
            }
            if order_only_change(&before, &after) {
                order_revisions = order_revisions.saturating_add(1);
            }
        }
        deltas.push(SemanticDelta {
            delta,
            changed,
            revision_delta: u64::from(order_revisions > 0),
        });
    }
    let subject = hex(&subject_datum(&facts, &closure, &package, &shape)?
        .content_id()
        .map_err(canonical)?
        .bytes);
    Ok(SemanticBatch {
        deltas,
        requests,
        effects,
        subprocesses,
        subject,
        facts: facts.len(),
    })
}

/// Reads [`read_semantics`] spends projecting every fact before and after each delta.
#[cfg(test)]
pub(crate) fn semantic_provider_reads(facts: usize) -> u64 {
    u64::try_from(facts)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::try_from(DELTAS.len()).unwrap_or(u64::MAX))
        .saturating_mul(2)
}

fn projection_boundary(value: &Datum) -> (u64, u64) {
    let Datum::Node { fields, .. } = value else {
        return (0, 0);
    };
    let effects = u64::try_from(
        fields
            .iter()
            .filter(|(name, _)| name.to_string() == "effect-id")
            .count(),
    )
    .unwrap_or(u64::MAX);
    let subprocesses = u64::try_from(
        fields
            .iter()
            .filter(|(name, _)| name.to_string() == "launcher")
            .count(),
    )
    .unwrap_or(u64::MAX);
    (effects, subprocesses)
}

pub(crate) fn judge(
    batch: &SemanticBatch,
    mutation: Option<GraphMutation>,
) -> Result<AssayObservation, WorldError> {
    let closure = seal_subject(mutation)?;
    let mut rows = Vec::with_capacity(batch.deltas.len());
    let mut observed_effects = 0u64;
    let mut observed_subprocesses = 0u64;
    for delta in &batch.deltas {
        let affected = closure
            .affected(delta.changed.iter().cloned())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut explanations = Vec::new();
        let mut planned_starts = 0usize;
        let mut journal_records = 0usize;
        let mut effects = 0u64;
        let mut subprocesses = 0u64;
        for conclusion in &affected {
            let boundary = crate::prove::attempt_leaf(conclusion.as_str(), delta.delta)?;
            planned_starts = planned_starts.saturating_add(boundary.attempts);
            journal_records = journal_records.saturating_add(boundary.journal_records);
            effects = effects.saturating_add(boundary.effects);
            subprocesses = subprocesses.saturating_add(boundary.subprocesses);
            for fact in &delta.changed {
                if let Ok(explanation) = closure.explain(conclusion, fact) {
                    explanations.push(explanation_value(explanation));
                }
            }
        }
        observed_effects = observed_effects.saturating_add(effects);
        observed_subprocesses = observed_subprocesses.saturating_add(subprocesses);
        rows.push(AssayRow {
            delta: delta.delta,
            changed: delta.changed.clone(),
            unaffected_basis_points: unaffected_basis_points(
                closure.conclusions().len(),
                affected.len(),
            ),
            revision_delta: delta.revision_delta,
            carried_state: false,
            planned_starts,
            journal_records,
            affected,
            explanations,
        });
    }
    if batch.effects > observed_effects || batch.subprocesses > observed_subprocesses {
        return Err(WorldError::Qualification(
            "projection claimed an effect the proof receipt did not record".to_owned(),
        ));
    }
    Ok(AssayObservation {
        denominator: closure.conclusions().len(),
        requests: batch.requests,
        effects: observed_effects,
        subprocesses: observed_subprocesses,
        subject: batch.subject.clone(),
        facts: batch.facts,
        rows,
    })
}

/// Assay subject stored on a cold proof and checked when its view is opened.
#[cfg(test)]
pub(crate) fn frozen_subject() -> Result<String, WorldError> {
    let facts = universe()?;
    let shape_id = provider::content_id(Datum::String("shape/world-config-v1".to_owned()))?;
    let requested = PackageIdentity {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        code: provider::content_id(Datum::String("world/semantic-text-caller".to_owned()))?,
    };
    let mut registry = ProjectionRegistry::new();
    let package = install_semantic_text_provider(&mut registry, shape_id.clone(), requested)?;
    let shape = WorldConfigShape { id: shape_id };
    let closure = seal_subject(None)?;
    Ok(hex(&subject_datum(&facts, &closure, &package, &shape)?
        .content_id()
        .map_err(canonical)?
        .bytes))
}

pub(crate) fn refuse_undeclared_read(conclusion: &str, facts: &[&str]) -> Result<(), WorldError> {
    let closure = seal_subject(None)?;
    let conclusion = ConclusionId::new(conclusion)?;
    for fact in facts {
        let fact = FactId::new(*fact)?;
        if !closure.depends_on(&conclusion, &fact) {
            return Err(WorldError::Qualification(format!(
                "hidden-dependency {conclusion} {fact}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn observation_record(observation: &AssayObservation) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "assay-observation-v1"),
        fields: vec![
            (
                Symbol::new("denominator"),
                count_datum(u64::try_from(observation.denominator).unwrap_or(u64::MAX)),
            ),
            (Symbol::new("requests"), count_datum(observation.requests)),
            (Symbol::new("effects"), count_datum(observation.effects)),
            (
                Symbol::new("subprocesses"),
                count_datum(observation.subprocesses),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (
                Symbol::new("subject"),
                Datum::String(observation.subject.clone()),
            ),
            (
                Symbol::new("facts"),
                count_datum(u64::try_from(observation.facts).unwrap_or(u64::MAX)),
            ),
            (
                Symbol::new("rows"),
                Datum::Vector(observation.rows.iter().map(row_datum).collect()),
            ),
        ],
    }
}

fn row_datum(row: &AssayRow) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "assay-row-v1"),
        fields: vec![
            (Symbol::new("delta"), Datum::String(row.delta.to_owned())),
            (
                Symbol::new("changed"),
                string_set(row.changed.iter().map(FactId::as_str)),
            ),
            (
                Symbol::new("affected"),
                string_set(row.affected.iter().map(ConclusionId::as_str)),
            ),
            (
                Symbol::new("unaffected-basis-points"),
                count_datum(u64::from(row.unaffected_basis_points)),
            ),
            (
                Symbol::new("revision-delta"),
                count_datum(row.revision_delta),
            ),
            (Symbol::new("carried-state"), Datum::Bool(row.carried_state)),
            (
                Symbol::new("explanations"),
                Datum::Vector(row.explanations.clone()),
            ),
            (
                Symbol::new("planned-starts"),
                count_datum(u64::try_from(row.planned_starts).unwrap_or(u64::MAX)),
            ),
            (
                Symbol::new("journal-records"),
                count_datum(u64::try_from(row.journal_records).unwrap_or(u64::MAX)),
            ),
        ],
    }
}

fn string_set<'a>(values: impl Iterator<Item = &'a str>) -> Datum {
    Datum::Vector(
        values
            .map(|value| Datum::String(value.to_owned()))
            .collect(),
    )
}

pub(crate) fn project_fact(
    engine: &ProjectionEngine<'_>,
    package: &PackageIdentity,
    shape: &WorldConfigShape,
    fact: &FactId,
    raw: &str,
    requests: &mut u64,
) -> Result<Datum, WorldError> {
    *requests = requests.saturating_add(1);
    let world = ObservedWorld::new([(
        fact.clone(),
        ObservedFact {
            semantic: Datum::String(raw.to_owned()),
            envelope: None,
        },
    )])?;
    let spec = ProjectionSpec {
        id: provider::content_id(Datum::String(format!("world/semantic-text/{fact}")))?,
        kind: ProjectionKindRef::new(SEMANTIC_TEXT_KIND)?,
        config: provider::config(),
        config_shape: shape.id.clone(),
        provider: package.clone(),
    };
    let policy = projector_policy(&shape.id, fact);
    Ok(engine.project(&world, &spec, &policy, None)?.projection)
}

fn projector_policy(shape: &sim_kernel::ContentId, fact: &FactId) -> ProjectorPolicy {
    ProjectorPolicy {
        input_shape: shape.clone(),
        reads: DeclaredInputSelector::new([fact.clone()]),
        imports: DeterministicImportManifest::default(),
        execution: ExecutionSemantics {
            id: "projection/trusted-native-v1".to_owned(),
            canonical_nan: true,
            canonical_collections: true,
            fresh_instance: true,
        },
        budgets: ProjectionBudget {
            max_inputs: 1,
            max_output_bytes: 64 * 1024,
            max_fuel: 1_000_000,
            max_memory_bytes: 4 * 1024 * 1024,
        },
        requires_confinement: false,
    }
}

pub(crate) fn seal_subject(
    mutation: Option<GraphMutation>,
) -> Result<FederatedClosure, WorldError> {
    let facts = universe()?;
    let mut by_owner = BTreeMap::<String, Vec<(ConclusionId, BTreeSet<FactId>)>>::new();
    for index in 1..=40 {
        let mut dependencies = BTreeSet::from([FactId::new(format!("stable/{index:02}"))?]);
        if let Some(changed) = scenario_fact(index) {
            dependencies.insert(FactId::new(changed)?);
        }
        match mutation {
            Some(GraphMutation::Declaration(target)) if target == index => {
                if scenario_fact(index).is_some() {
                    continue;
                }
                dependencies.insert(FactId::new("change/private-implementation")?);
            }
            Some(GraphMutation::Edge(target, edge)) if target == index => {
                let original = dependencies.iter().nth(edge).cloned().ok_or_else(|| {
                    WorldError::Qualification("edge mutant is out of range".to_owned())
                })?;
                dependencies.remove(&original);
                if original.as_str().starts_with("change/") {
                    dependencies.insert(FactId::new("change/dormant")?);
                } else {
                    let replacement = CHANGE_FACTS
                        .into_iter()
                        .find(|candidate| {
                            dependencies
                                .iter()
                                .all(|present| present.as_str() != *candidate)
                        })
                        .ok_or_else(|| {
                            WorldError::Qualification("edge mutant has no replacement".to_owned())
                        })?;
                    dependencies.insert(FactId::new(replacement)?);
                }
            }
            _ => {}
        }
        by_owner.entry(owner_name(index)).or_default().push((
            ConclusionId::new(format!("conclusion/{index:02}"))?,
            dependencies,
        ));
    }
    let graphs = by_owner
        .into_iter()
        .map(|(owner, dependencies)| OwnerProjectionGraph::new(owner, dependencies))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    FederatedClosure::seal(facts, graphs)
        .map_err(|error| WorldError::Qualification(error.to_string()))
}

pub(crate) fn universe() -> Result<Vec<FactId>, WorldError> {
    let mut names = Vec::new();
    for index in 1..=40 {
        names.push(format!("stable/{index:02}"));
    }
    names.extend(CHANGE_FACTS.iter().map(|fact| (*fact).to_owned()));
    names.push("change/dormant".to_owned());
    names.push("present/order".to_owned());
    names
        .into_iter()
        .map(|name| Ok(FactId::new(name)?))
        .collect()
}

pub(crate) fn owner_name(index: usize) -> String {
    format!("owner/{:02}", index.saturating_sub(1) / 10 + 1)
}

fn conclusion_index(name: &str) -> usize {
    name.rsplit_once('/')
        .and_then(|(_, index)| index.parse().ok())
        .unwrap_or(1)
}

fn scenario_fact(index: usize) -> Option<&'static str> {
    match index {
        1 | 2 => Some("change/private-implementation"),
        3..=10 => Some("change/public-api"),
        11..=13 => Some("change/head-a"),
        14..=16 => Some("change/head-b"),
        17..=19 => Some("change/head-c"),
        20..=22 => Some("change/control-pins"),
        23 | 24 => Some("change/proof-code-alpha"),
        25 | 26 => Some("change/checker-alpha"),
        _ => None,
    }
}

pub(crate) fn baseline_bytes(fact: &str) -> String {
    if fact == "present/order" {
        "beta\nalpha".to_owned()
    } else {
        format!("baseline {fact}")
    }
}

pub(crate) fn variant_bytes(delta: &str, fact: &str) -> String {
    match (delta, fact) {
        ("D1", "stable/40") => "baseline stable/40 ".to_owned(),
        ("D2", "change/private-implementation") => "edited private implementation".to_owned(),
        ("D3", "change/public-api") => "edited public api".to_owned(),
        ("D4", "change/head-a" | "change/head-b" | "change/head-c" | "change/control-pins") => {
            format!("edited {fact}")
        }
        ("D5", "change/proof-code-alpha" | "change/checker-alpha") => format!("edited {fact}"),
        ("D6", "present/order") => "alpha\nbeta".to_owned(),
        _ => baseline_bytes(fact),
    }
}

fn order_only_change(before: &str, after: &str) -> bool {
    let left = trimmed_lines(before);
    let right = trimmed_lines(after);
    if left == right {
        return false;
    }
    let mut left_sorted = left;
    let mut right_sorted = right;
    left_sorted.sort_unstable();
    right_sorted.sort_unstable();
    left_sorted == right_sorted
}

fn trimmed_lines(raw: &str) -> Vec<&str> {
    raw.split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn subject_datum(
    facts: &[FactId],
    closure: &FederatedClosure,
    package: &PackageIdentity,
    shape: &WorldConfigShape,
) -> Result<Datum, WorldError> {
    let mut rows = Vec::with_capacity(facts.len());
    for fact in facts {
        rows.push(Datum::Node {
            tag: Symbol::qualified("world", "assay-fact-v1"),
            fields: vec![
                (Symbol::new("id"), Datum::String(fact.as_str().to_owned())),
                (
                    Symbol::new("bytes"),
                    Datum::String(baseline_bytes(fact.as_str())),
                ),
            ],
        });
        for delta in DELTAS {
            rows.push(Datum::String(format!(
                "postimage {delta} {} {}",
                fact.as_str(),
                variant_bytes(delta, fact.as_str())
            )));
        }
    }
    let mut graph = Vec::new();
    for conclusion in closure.conclusions() {
        let deps = facts
            .iter()
            .filter(|fact| closure.depends_on(conclusion, fact))
            .map(|fact| fact.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let owner = owner_name(conclusion_index(conclusion.as_str()));
        graph.push(format!("graph {owner} {} {deps}", conclusion.as_str()));
    }
    rows.extend(crate::assay_oracle::closure_rows(&graph));
    let sample = facts
        .first()
        .ok_or_else(|| WorldError::Qualification("commissioned assay is empty".to_owned()))?;
    rows.extend(identity_rows(package, shape, sample)?);
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "assay-baseline-v1"),
        fields: vec![(Symbol::new("facts"), Datum::Vector(rows))],
    })
}

/// Text bound into the assay subject, for the freeze check.
#[cfg(test)]
pub(crate) fn subject_rows() -> Result<Vec<String>, WorldError> {
    let facts = universe()?;
    let closure = seal_subject(None)?;
    let shape_id = provider::content_id(Datum::String("shape/world-config-v1".to_owned()))?;
    let requested = PackageIdentity {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        code: provider::content_id(Datum::String("world/semantic-text-caller".to_owned()))?,
    };
    let mut registry = ProjectionRegistry::new();
    let package = install_semantic_text_provider(&mut registry, shape_id.clone(), requested)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let shape = WorldConfigShape { id: shape_id };
    let datum = subject_datum(&facts, &closure, &package, &shape)?;
    let mut rows = Vec::new();
    flatten(&datum, &mut rows);
    Ok(rows)
}

fn identity_rows(
    package: &PackageIdentity,
    shape: &WorldConfigShape,
    fact: &FactId,
) -> Result<Vec<Datum>, WorldError> {
    let policy = projector_policy(&shape.id, fact);
    let mut rows = vec![
        Datum::String(format!("provider {} {}", package.name, package.version)),
        Datum::String(format!("provider-code {}", hex(&package.code.bytes))),
        Datum::String(format!("config-shape {}", hex(&shape.id.bytes))),
        Datum::String("checker sim-incremental-core/projection/semantic-text".to_owned()),
        Datum::String(format!("provider-kind {SEMANTIC_TEXT_KIND}")),
        Datum::String(format!(
            "execution {} canonical_nan {} canonical_collections {} fresh_instance {}",
            policy.execution.id,
            policy.execution.canonical_nan,
            policy.execution.canonical_collections,
            policy.execution.fresh_instance
        )),
        Datum::String(format!(
            "budget max_inputs {} max_output_bytes {} max_fuel {} max_memory_bytes {}",
            policy.budgets.max_inputs,
            policy.budgets.max_output_bytes,
            policy.budgets.max_fuel,
            policy.budgets.max_memory_bytes
        )),
        Datum::String(format!(
            "requires_confinement {}",
            policy.requires_confinement
        )),
    ];
    if policy.imports.imports.is_empty() {
        rows.push(Datum::String("imports none".to_owned()));
    }
    Ok(rows)
}

#[cfg(test)]
fn flatten(datum: &Datum, rows: &mut Vec<String>) {
    match datum {
        Datum::String(text) => rows.push(text.clone()),
        Datum::Vector(values) => {
            for value in values {
                flatten(value, rows);
            }
        }
        Datum::Node { fields, .. } => {
            for (_, value) in fields {
                flatten(value, rows);
            }
        }
        _ => {}
    }
}

fn unaffected_basis_points(denominator: usize, affected: usize) -> u16 {
    if denominator == 0 || affected > denominator {
        return 0;
    }
    u16::try_from((denominator - affected).saturating_mul(10_000) / denominator).unwrap_or(10_000)
}

fn canonical(error: impl ToString) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
