//! Pure, domain-neutral evidence aggregation and decision projections.
//!
//! This is deliberately downstream of statistics and the study journal. It
//! consumes fixed evidence, seals every policy input, and produces one report
//! graph from which all human and machine views are rendered.

use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};
use sim_study_core::{EstimateRecord, EvidenceClass};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DecisionError {
    #[error("unsupported number domain")]
    UnsupportedNumberDomain,
    #[error("non-finite statistic")]
    NonFiniteStatistic,
    #[error("incompatible evidence")]
    IncompatibleEvidence,
    #[error("missing required evidence")]
    InsufficientEvidence,
    #[error("invalid decision specification")]
    InvalidSpec,
    #[error("selection contains inadmissible evidence")]
    InadmissibleSelection,
    #[error("selection contains an unresolved subject")]
    UnresolvedSelection,
    #[error("noncanonical report datum")]
    Noncanonical,
}

/// Complete identity-bearing invocation of an upstream statistical operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceCall {
    pub operation: Symbol,
    pub implementation: ContentId,
    pub inputs: Vec<ContentId>,
    pub parameters: BTreeMap<Symbol, NumberLiteral>,
}

impl InferenceCall {
    pub fn content_id(&self) -> Result<ContentId, DecisionError> {
        self.to_datum()
            .content_id()
            .map_err(|_| DecisionError::Noncanonical)
    }

    fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: sym("inference-call"),
            fields: vec![
                (sym("operation"), Datum::Symbol(self.operation.clone())),
                (sym("implementation"), cid(&self.implementation)),
                (
                    sym("inputs"),
                    Datum::Vector(self.inputs.iter().map(cid).collect()),
                ),
                (
                    sym("parameters"),
                    Datum::Map(
                        self.parameters
                            .iter()
                            .map(|(key, value)| {
                                (Datum::Symbol(key.clone()), Datum::Number(value.clone()))
                            })
                            .collect(),
                    ),
                ),
            ],
        }
    }
}

/// The only adapter in this crate from statistics' machine `f64` to a
/// canonical study estimate. Only the canonical IEEE binary64 domain is
/// accepted; callers cannot silently relabel a result as money or a count.
pub fn statistic_estimate(
    domain: &Symbol,
    point: f64,
    lower: f64,
    upper: f64,
    confidence: f64,
    call: &InferenceCall,
    evidence: EvidenceClass,
) -> Result<EstimateRecord, DecisionError> {
    if domain != &sym("binary64") {
        return Err(DecisionError::UnsupportedNumberDomain);
    }
    let number = |value: f64| {
        if !value.is_finite() {
            return Err(DecisionError::NonFiniteStatistic);
        }
        Ok(NumberLiteral {
            domain: domain.clone(),
            canonical: canonical_f64(value),
        })
    };
    EstimateRecord::new(
        number(point)?,
        number(lower)?,
        number(upper)?,
        number(confidence)?,
        call.content_id()?,
        evidence,
    )
    .map_err(|_| DecisionError::IncompatibleEvidence)
}

fn canonical_f64(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FacetSample {
    pub evidence: ContentId,
    pub subject: ContentId,
    pub facet: Symbol,
    pub route: ContentId,
    pub environment: ContentId,
    pub value: Option<f64>,
    pub interval: Option<(f64, f64)>,
    pub censored: bool,
    pub failed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FacetAggregate {
    pub subject: ContentId,
    pub facet: Symbol,
    pub observed: u64,
    pub censored: u64,
    pub failures: u64,
    pub point: Option<f64>,
    pub interval: Option<(f64, f64)>,
    pub dispersion: Option<f64>,
    pub strata: BTreeMap<(ContentId, ContentId), u64>,
    pub provenance: BTreeSet<ContentId>,
}

/// Aggregates only equal subject/facet cells. Route and environment remain
/// explicit strata rather than being averaged out.
pub fn aggregate_facets(samples: &[FacetSample]) -> Result<Vec<FacetAggregate>, DecisionError> {
    let mut groups = BTreeMap::<(ContentId, Symbol), Vec<&FacetSample>>::new();
    for sample in samples {
        if let Some(value) = sample.value
            && !value.is_finite()
        {
            return Err(DecisionError::NonFiniteStatistic);
        }
        groups
            .entry((sample.subject.clone(), sample.facet.clone()))
            .or_default()
            .push(sample);
    }
    Ok(groups
        .into_iter()
        .map(|((subject, facet), rows)| {
            let values = rows.iter().filter_map(|row| row.value).collect::<Vec<_>>();
            let point =
                (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64);
            let dispersion = point.map(|mean| {
                (values
                    .iter()
                    .map(|value| (value - mean).powi(2))
                    .sum::<f64>()
                    / values.len() as f64)
                    .sqrt()
            });
            let lowers = rows.iter().filter_map(|row| row.interval.map(|v| v.0));
            let uppers = rows.iter().filter_map(|row| row.interval.map(|v| v.1));
            let lower = lowers.reduce(f64::min);
            let upper = uppers.reduce(f64::max);
            let mut strata = BTreeMap::new();
            let mut provenance = BTreeSet::new();
            for row in &rows {
                *strata
                    .entry((row.route.clone(), row.environment.clone()))
                    .or_insert(0) += 1;
                provenance.insert(row.evidence.clone());
            }
            FacetAggregate {
                subject,
                facet,
                observed: values.len() as u64,
                censored: rows.iter().filter(|row| row.censored).count() as u64,
                failures: rows.iter().filter(|row| row.failed).count() as u64,
                point,
                interval: lower.zip(upper),
                dispersion,
                strata,
                provenance,
            }
        })
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContrastKind {
    Causal,
    Descriptive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedTreatment {
    pub left_treatment: ContentId,
    pub right_treatment: ContentId,
    pub pair_keys: BTreeSet<ContentId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contrast {
    pub left: ContentId,
    pub right: ContentId,
    pub difference: f64,
    pub kind: ContrastKind,
    pub pairs: BTreeSet<ContentId>,
}

pub fn contrast(
    left: ContentId,
    right: ContentId,
    left_value: f64,
    right_value: f64,
    declared_pair: Option<&PairedTreatment>,
) -> Result<Contrast, DecisionError> {
    if !left_value.is_finite() || !right_value.is_finite() {
        return Err(DecisionError::NonFiniteStatistic);
    }
    let pairs = declared_pair
        .filter(|pair| pair.left_treatment == left && pair.right_treatment == right)
        .map(|pair| pair.pair_keys.clone())
        .unwrap_or_default();
    Ok(Contrast {
        left,
        right,
        difference: right_value - left_value,
        kind: if pairs.is_empty() {
            ContrastKind::Descriptive
        } else {
            ContrastKind::Causal
        },
        pairs,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceCause {
    CandidateRoute,
    Observer,
    Shared,
    Unattributed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributedResourceEvent {
    pub evidence: ContentId,
    pub subject: Option<ContentId>,
    pub route: Option<ContentId>,
    pub resource: Symbol,
    pub amount: u64,
    pub cause: ResourceCause,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendSummary {
    pub total: u64,
    pub comparable: u64,
    pub by_cause: BTreeMap<ResourceCause, u64>,
    pub provenance: BTreeSet<ContentId>,
    pub economic_censored: bool,
}

pub fn summarize_spend(events: &[AttributedResourceEvent]) -> SpendSummary {
    let mut out = SpendSummary {
        total: 0,
        comparable: 0,
        by_cause: BTreeMap::new(),
        provenance: BTreeSet::new(),
        economic_censored: false,
    };
    for event in events {
        out.total = out.total.saturating_add(event.amount);
        *out.by_cause.entry(event.cause).or_insert(0) += event.amount;
        if matches!(
            event.cause,
            ResourceCause::CandidateRoute | ResourceCause::Shared
        ) {
            out.comparable = out.comparable.saturating_add(event.amount);
        }
        if matches!(
            event.cause,
            ResourceCause::Observer | ResourceCause::Unattributed
        ) {
            out.economic_censored = true;
        }
        out.provenance.insert(event.evidence.clone());
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct EconomicResult {
    pub total_spend: u64,
    pub comparable_spend: u64,
    pub accepted: u64,
    pub cost_per_accepted: Option<f64>,
    pub verdict_censored: bool,
}

pub fn cost_per_accepted(spend: &SpendSummary, accepted: u64) -> EconomicResult {
    EconomicResult {
        total_spend: spend.total,
        comparable_spend: spend.comparable,
        accepted,
        cost_per_accepted: (accepted != 0).then(|| spend.comparable as f64 / accepted as f64),
        verdict_censored: spend.economic_censored,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Minimize,
    Maximize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DimensionSpec {
    pub name: Symbol,
    pub direction: Direction,
    pub equivalence: f64,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecisionSpec {
    pub id: ContentId,
    pub evidence_root: ContentId,
    pub dimensions: Vec<DimensionSpec>,
    pub budget: Option<u64>,
    pub negative_epoch_floor: u32,
    pub expiry: ContentId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubjectEvidence {
    pub subject: ContentId,
    pub values: BTreeMap<Symbol, f64>,
    pub evidence: BTreeSet<ContentId>,
    pub inferences: BTreeSet<ContentId>,
    pub attributions: BTreeSet<ContentId>,
    pub epochs: BTreeSet<ContentId>,
    pub spend: SpendSummary,
    pub gate_rejected: bool,
    pub unresolved: bool,
    pub bootstrap: bool,
    pub private_unapproved: bool,
    pub report_only: bool,
    pub quarantined: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    InsufficientEvidence,
    RejectedGate,
    OverBudget,
    Dominated,
    Eligible,
    Preferred,
    Incomparable,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relation {
    Dominates,
    Equivalent,
    Incomparable,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecisionEdge {
    pub left: ContentId,
    pub right: ContentId,
    pub relation: Relation,
    pub dimensions: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectDecision {
    pub subject: ContentId,
    pub verdict: Verdict,
    pub decisive_evidence: BTreeSet<ContentId>,
    pub decisive_inferences: BTreeSet<ContentId>,
    pub decisive_attributions: BTreeSet<ContentId>,
    pub reasons: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportGraph {
    pub id: ContentId,
    pub spec: DecisionSpec,
    pub subjects: Vec<SubjectDecision>,
    pub edges: Vec<DecisionEdge>,
    pub equivalence_classes: Vec<Vec<ContentId>>,
    pub tiers: Vec<Vec<ContentId>>,
    pub evidence_root: ContentId,
}

pub fn decide(
    spec: DecisionSpec,
    mut evidence: Vec<SubjectEvidence>,
) -> Result<ReportGraph, DecisionError> {
    validate_spec(&spec)?;
    evidence.sort_by(|a, b| a.subject.cmp(&b.subject));
    let mut edges = Vec::new();
    for left in 0..evidence.len() {
        for right in left + 1..evidence.len() {
            edges.push(compare(&spec, &evidence[left], &evidence[right]));
        }
    }
    let mut decisions = evidence
        .iter()
        .map(|row| initial_verdict(&spec, row))
        .collect::<Vec<_>>();
    for edge in &edges {
        if edge.relation == Relation::Dominates {
            let dominated = decisions
                .iter_mut()
                .find(|row| row.subject == edge.right)
                .expect("known subject");
            if dominated.verdict >= Verdict::Eligible {
                dominated.verdict = Verdict::Dominated;
                dominated.reasons.push(sym("proven-dominance"));
            }
        }
    }
    let eligible = decisions
        .iter()
        .filter(|row| row.verdict == Verdict::Eligible)
        .map(|row| row.subject.clone())
        .collect::<Vec<_>>();
    if eligible.len() == 1 {
        decisions
            .iter_mut()
            .find(|row| row.subject == eligible[0])
            .expect("known subject")
            .verdict = Verdict::Preferred;
    } else if eligible.len() > 1 {
        for subject in eligible {
            let has_incomparable = edges.iter().any(|edge| {
                edge.relation == Relation::Incomparable
                    && (edge.left == subject || edge.right == subject)
            });
            if has_incomparable {
                decisions
                    .iter_mut()
                    .find(|row| row.subject == subject)
                    .expect("known subject")
                    .verdict = Verdict::Incomparable;
            }
        }
    }
    let equivalence_classes = equivalence_classes(&evidence, &edges);
    let tiers = conservative_tiers(&decisions, &edges);
    let mut graph = ReportGraph {
        id: spec.id.clone(),
        evidence_root: spec.evidence_root.clone(),
        spec,
        subjects: decisions,
        edges,
        equivalence_classes,
        tiers,
    };
    graph.id = graph
        .to_datum()
        .content_id()
        .map_err(|_| DecisionError::Noncanonical)?;
    Ok(graph)
}

fn validate_spec(spec: &DecisionSpec) -> Result<(), DecisionError> {
    let names = spec
        .dimensions
        .iter()
        .map(|d| d.name.clone())
        .collect::<BTreeSet<_>>();
    if spec.dimensions.is_empty()
        || names.len() != spec.dimensions.len()
        || spec
            .dimensions
            .iter()
            .any(|d| !d.equivalence.is_finite() || d.equivalence < 0.0)
    {
        Err(DecisionError::InvalidSpec)
    } else {
        Ok(())
    }
}

fn initial_verdict(spec: &DecisionSpec, row: &SubjectEvidence) -> SubjectDecision {
    let missing = spec
        .dimensions
        .iter()
        .any(|dimension| dimension.required && !row.values.contains_key(&dimension.name));
    let (verdict, reason) = if missing || row.unresolved {
        (Verdict::InsufficientEvidence, "missing-required-evidence")
    } else if row.gate_rejected {
        (Verdict::RejectedGate, "rejected-gate")
    } else if spec
        .budget
        .is_some_and(|budget| row.spend.comparable > budget)
        && !row.spend.economic_censored
    {
        (Verdict::OverBudget, "over-budget")
    } else {
        (Verdict::Eligible, "eligible")
    };
    SubjectDecision {
        subject: row.subject.clone(),
        verdict,
        decisive_evidence: row.evidence.clone(),
        decisive_inferences: row.inferences.clone(),
        decisive_attributions: row.attributions.clone(),
        reasons: vec![sym(reason)],
    }
}

fn compare(spec: &DecisionSpec, left: &SubjectEvidence, right: &SubjectEvidence) -> DecisionEdge {
    let mut left_better = false;
    let mut right_better = false;
    let mut compared = Vec::new();
    for dimension in &spec.dimensions {
        let (Some(a), Some(b)) = (
            left.values.get(&dimension.name),
            right.values.get(&dimension.name),
        ) else {
            continue;
        };
        compared.push(dimension.name.clone());
        if (a - b).abs() <= dimension.equivalence {
            continue;
        }
        let a_better = match dimension.direction {
            Direction::Minimize => a < b,
            Direction::Maximize => a > b,
        };
        left_better |= a_better;
        right_better |= !a_better;
    }
    let relation = match (left_better, right_better, compared.is_empty()) {
        (_, _, true) | (true, true, false) => Relation::Incomparable,
        (false, false, false) => Relation::Equivalent,
        (true, false, false) => Relation::Dominates,
        (false, true, false) => Relation::Dominates,
    };
    let (left_id, right_id) = if relation == Relation::Dominates && right_better {
        (&right.subject, &left.subject)
    } else {
        (&left.subject, &right.subject)
    };
    DecisionEdge {
        left: left_id.clone(),
        right: right_id.clone(),
        relation,
        dimensions: compared,
    }
}

fn equivalence_classes(
    evidence: &[SubjectEvidence],
    edges: &[DecisionEdge],
) -> Vec<Vec<ContentId>> {
    let mut classes = evidence
        .iter()
        .map(|row| vec![row.subject.clone()])
        .collect::<Vec<_>>();
    for edge in edges
        .iter()
        .filter(|edge| edge.relation == Relation::Equivalent)
    {
        let a = classes
            .iter()
            .position(|class| class.contains(&edge.left))
            .unwrap();
        let b = classes
            .iter()
            .position(|class| class.contains(&edge.right))
            .unwrap();
        if a != b {
            let removed = classes.remove(b);
            let target = if b < a { a - 1 } else { a };
            classes[target].extend(removed);
            classes[target].sort();
        }
    }
    classes.sort();
    classes
}

fn conservative_tiers(
    decisions: &[SubjectDecision],
    edges: &[DecisionEdge],
) -> Vec<Vec<ContentId>> {
    let mut remaining = decisions
        .iter()
        .filter(|row| row.verdict >= Verdict::Eligible)
        .map(|row| row.subject.clone())
        .collect::<BTreeSet<_>>();
    let mut tiers = Vec::new();
    while !remaining.is_empty() {
        let tier = remaining
            .iter()
            .filter(|subject| {
                !edges.iter().any(|edge| {
                    edge.relation == Relation::Dominates
                        && &edge.right == *subject
                        && remaining.contains(&edge.left)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if tier.is_empty() {
            break;
        }
        for subject in &tier {
            remaining.remove(subject);
        }
        tiers.push(tier);
    }
    tiers
}

/// A negative projection is never quality-bearing from the first independent
mod selection;

pub use selection::*;
use selection::{cid, sym};

#[cfg(test)]
mod tests;
