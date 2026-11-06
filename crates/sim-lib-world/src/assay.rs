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
    crate::assay_mutant::ensure_mutants_diverge(&batch, &sound)?;
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
    let subject = hex(&subject_datum(&facts)?
        .content_id()
        .map_err(canonical)?
        .bytes);
    Ok(SemanticBatch {
        deltas,
        requests,
        effects: 0,
        subprocesses: 0,
        subject,
        facts: facts.len(),
    })
}

pub(crate) fn judge(
    batch: &SemanticBatch,
    mutation: Option<GraphMutation>,
) -> Result<AssayObservation, WorldError> {
    let closure = seal_subject(mutation)?;
    let mut rows = Vec::with_capacity(batch.deltas.len());
    for delta in &batch.deltas {
        let affected = closure
            .affected(delta.changed.iter().cloned())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut explanations = Vec::new();
        for conclusion in &affected {
            for fact in &delta.changed {
                if let Ok(explanation) = closure.explain(conclusion, fact) {
                    explanations.push(explanation_value(explanation));
                }
            }
        }
        let semantic = !delta.changed.is_empty();
        rows.push(AssayRow {
            delta: delta.delta,
            changed: delta.changed.clone(),
            unaffected_basis_points: unaffected_basis_points(
                closure.conclusions().len(),
                affected.len(),
            ),
            revision_delta: delta.revision_delta,
            carried_state: false,
            planned_starts: affected.len(),
            journal_records: if semantic { delta.changed.len() } else { 0 },
            affected,
            explanations,
        });
    }
    Ok(AssayObservation {
        denominator: closure.conclusions().len(),
        requests: batch.requests,
        effects: batch.effects,
        subprocesses: batch.subprocesses,
        subject: batch.subject.clone(),
        facts: batch.facts,
        rows,
    })
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
    let policy = ProjectorPolicy {
        input_shape: shape.id.clone(),
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
    };
    Ok(engine.project(&world, &spec, &policy, None)?.projection)
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
        by_owner
            .entry(format!("owner/{:02}", (index - 1) / 10 + 1))
            .or_default()
            .push((
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

fn subject_datum(facts: &[FactId]) -> Result<Datum, WorldError> {
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
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "assay-baseline-v1"),
        fields: vec![(Symbol::new("facts"), Datum::Vector(rows))],
    })
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
