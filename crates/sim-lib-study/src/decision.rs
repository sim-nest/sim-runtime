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
/// epoch, and its required floor is sealed by the decision specification.
pub fn negative_projection_allowed(spec: &DecisionSpec, epochs: &BTreeSet<ContentId>) -> bool {
    epochs.len() >= usize::max(2, spec.negative_epoch_floor as usize)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StalenessInputs {
    pub subject_snapshot: ContentId,
    pub evidence_root: ContentId,
    pub policy: ContentId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub id: ContentId,
    pub subjects: Vec<ContentId>,
    pub decisive_evidence: BTreeSet<ContentId>,
    pub report_root: ContentId,
    pub expiry: ContentId,
    pub staleness: StalenessInputs,
}

pub fn select(
    graph: &ReportGraph,
    evidence: &[SubjectEvidence],
    staleness: StalenessInputs,
) -> Result<Selection, DecisionError> {
    let by_subject = evidence
        .iter()
        .map(|row| (row.subject.clone(), row))
        .collect::<BTreeMap<_, _>>();
    let subjects = graph
        .subjects
        .iter()
        .filter(|row| matches!(row.verdict, Verdict::Preferred | Verdict::Incomparable))
        .map(|row| row.subject.clone())
        .collect::<Vec<_>>();
    if subjects.is_empty() {
        return Err(DecisionError::UnresolvedSelection);
    }
    let mut decisive_evidence = BTreeSet::new();
    for subject in &subjects {
        let row = by_subject
            .get(subject)
            .ok_or(DecisionError::UnresolvedSelection)?;
        if row.bootstrap
            || row.private_unapproved
            || row.report_only
            || row.unresolved
            || row.quarantined
        {
            return Err(DecisionError::InadmissibleSelection);
        }
        decisive_evidence.extend(row.evidence.iter().cloned());
    }
    let mut selection = Selection {
        id: graph.id.clone(),
        subjects,
        decisive_evidence,
        report_root: graph.id.clone(),
        expiry: graph.spec.expiry.clone(),
        staleness,
    };
    selection.id = selection
        .to_datum()
        .content_id()
        .map_err(|_| DecisionError::Noncanonical)?;
    Ok(selection)
}

impl ReportGraph {
    pub fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: sym("decision-report"),
            fields: vec![
                (sym("spec"), cid(&self.spec.id)),
                (sym("evidence-root"), cid(&self.evidence_root)),
                (
                    sym("subjects"),
                    Datum::Vector(
                        self.subjects
                            .iter()
                            .map(|row| Datum::Node {
                                tag: sym("subject"),
                                fields: vec![
                                    (sym("id"), cid(&row.subject)),
                                    (
                                        sym("verdict"),
                                        Datum::Symbol(sym(verdict_name(row.verdict))),
                                    ),
                                    (
                                        sym("evidence"),
                                        Datum::Vector(
                                            row.decisive_evidence.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("inferences"),
                                        Datum::Vector(
                                            row.decisive_inferences.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("attributions"),
                                        Datum::Vector(
                                            row.decisive_attributions.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("reasons"),
                                        Datum::Vector(
                                            row.reasons
                                                .iter()
                                                .cloned()
                                                .map(Datum::Symbol)
                                                .collect(),
                                        ),
                                    ),
                                ],
                            })
                            .collect(),
                    ),
                ),
                (
                    sym("edges"),
                    Datum::Vector(
                        self.edges
                            .iter()
                            .map(|edge| Datum::Node {
                                tag: sym("edge"),
                                fields: vec![
                                    (sym("left"), cid(&edge.left)),
                                    (sym("right"), cid(&edge.right)),
                                    (
                                        sym("relation"),
                                        Datum::Symbol(sym(relation_name(&edge.relation))),
                                    ),
                                    (
                                        sym("dimensions"),
                                        Datum::Vector(
                                            edge.dimensions
                                                .iter()
                                                .cloned()
                                                .map(Datum::Symbol)
                                                .collect(),
                                        ),
                                    ),
                                ],
                            })
                            .collect(),
                    ),
                ),
                (
                    sym("equivalence-classes"),
                    Datum::Vector(
                        self.equivalence_classes
                            .iter()
                            .map(|class| Datum::Vector(class.iter().map(cid).collect()))
                            .collect(),
                    ),
                ),
                (
                    sym("tiers"),
                    Datum::Vector(
                        self.tiers
                            .iter()
                            .map(|tier| Datum::Vector(tier.iter().map(cid).collect()))
                            .collect(),
                    ),
                ),
            ],
        }
    }

    pub fn render_sim(&self) -> String {
        render_datum(&self.to_datum())
    }
    pub fn render_markdown(&self) -> String {
        let mut out = String::from(
            "| subject | verdict | evidence | inference | attribution |\n|---|---|---|---|---|\n",
        );
        for row in &self.subjects {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                short(&row.subject),
                verdict_name(row.verdict),
                row.decisive_evidence
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_inferences
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_attributions
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";")
            ));
        }
        out
    }
    pub fn render_csv(&self) -> String {
        let mut out = String::from("subject,verdict,evidence,inference,attribution\n");
        for row in &self.subjects {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                short(&row.subject),
                verdict_name(row.verdict),
                row.decisive_evidence
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_inferences
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_attributions
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";")
            ));
        }
        out
    }
    pub fn render_table(&self) -> Vec<Vec<String>> {
        self.subjects
            .iter()
            .map(|row| {
                vec![
                    short(&row.subject),
                    verdict_name(row.verdict).into(),
                    row.decisive_evidence
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                    row.decisive_inferences
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                    row.decisive_attributions
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                ]
            })
            .collect()
    }
}

impl Selection {
    fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: sym("selection"),
            fields: vec![
                (
                    sym("subjects"),
                    Datum::Vector(self.subjects.iter().map(cid).collect()),
                ),
                (
                    sym("evidence"),
                    Datum::Vector(self.decisive_evidence.iter().map(cid).collect()),
                ),
                (sym("report-root"), cid(&self.report_root)),
                (sym("expiry"), cid(&self.expiry)),
                (
                    sym("subject-snapshot"),
                    cid(&self.staleness.subject_snapshot),
                ),
                (sym("evidence-root"), cid(&self.staleness.evidence_root)),
                (sym("policy"), cid(&self.staleness.policy)),
            ],
        }
    }
}

fn verdict_name(value: Verdict) -> &'static str {
    match value {
        Verdict::InsufficientEvidence => "insufficient-evidence",
        Verdict::RejectedGate => "rejected-gate",
        Verdict::OverBudget => "over-budget",
        Verdict::Dominated => "dominated",
        Verdict::Eligible => "eligible",
        Verdict::Preferred => "preferred",
        Verdict::Incomparable => "incomparable",
    }
}
fn relation_name(value: &Relation) -> &'static str {
    match value {
        Relation::Dominates => "dominates",
        Relation::Equivalent => "equivalent",
        Relation::Incomparable => "incomparable",
    }
}
fn sym(name: &str) -> Symbol {
    Symbol::qualified("study", name)
}
fn cid(id: &ContentId) -> Datum {
    Datum::Node {
        tag: sym("content-id"),
        fields: vec![
            (sym("algorithm"), Datum::Symbol(id.algorithm.clone())),
            (sym("digest"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}
fn short(id: &ContentId) -> String {
    id.bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn render_datum(datum: &Datum) -> String {
    format!("{datum:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn id(name: &str) -> ContentId {
        ContentId::from_bytes(sym(name), Sha256::digest(name).into())
    }
    fn spend(comparable: u64) -> SpendSummary {
        summarize_spend(&[AttributedResourceEvent {
            evidence: id("spend"),
            subject: None,
            route: None,
            resource: sym("tokens"),
            amount: comparable,
            cause: ResourceCause::CandidateRoute,
        }])
    }
    fn row(name: &str, quality: Option<f64>, cost: Option<f64>, epochs: usize) -> SubjectEvidence {
        let mut values = BTreeMap::new();
        if let Some(value) = quality {
            values.insert(sym("quality"), value);
        }
        if let Some(value) = cost {
            values.insert(sym("cost"), value);
        }
        SubjectEvidence {
            subject: id(name),
            values,
            evidence: BTreeSet::from([id(&format!("evidence-{name}"))]),
            inferences: BTreeSet::from([id(&format!("inference-{name}"))]),
            attributions: BTreeSet::from([id(&format!("attribution-{name}"))]),
            epochs: (0..epochs)
                .map(|n| id(&format!("epoch-{name}-{n}")))
                .collect(),
            spend: spend(cost.unwrap_or(0.0) as u64),
            gate_rejected: false,
            unresolved: false,
            bootstrap: false,
            private_unapproved: false,
            report_only: false,
            quarantined: false,
        }
    }
    fn spec() -> DecisionSpec {
        DecisionSpec {
            id: id("spec"),
            evidence_root: id("root"),
            dimensions: vec![
                DimensionSpec {
                    name: sym("quality"),
                    direction: Direction::Maximize,
                    equivalence: 0.01,
                    required: true,
                },
                DimensionSpec {
                    name: sym("cost"),
                    direction: Direction::Minimize,
                    equivalence: 0.01,
                    required: true,
                },
            ],
            budget: Some(100),
            negative_epoch_floor: 2,
            expiry: id("expiry"),
        }
    }

    #[test]
    fn f64_adapter_stamps_complete_call_and_refuses_other_domains() {
        let call = InferenceCall {
            operation: sym("mean-interval"),
            implementation: id("stats-v1"),
            inputs: vec![id("a"), id("b")],
            parameters: BTreeMap::new(),
        };
        let estimate = statistic_estimate(
            &sym("binary64"),
            1.0,
            0.5,
            1.5,
            0.95,
            &call,
            EvidenceClass::Publishable,
        )
        .unwrap();
        assert_eq!(estimate.inference, call.content_id().unwrap());
        assert_eq!(estimate.point.canonical, "1");
        assert_eq!(
            statistic_estimate(
                &sym("decimal"),
                1.0,
                0.0,
                2.0,
                0.95,
                &call,
                EvidenceClass::Publishable
            ),
            Err(DecisionError::UnsupportedNumberDomain)
        );
    }

    #[test]
    fn aggregation_preserves_strata_counts_failures_and_provenance() {
        let samples = vec![
            FacetSample {
                evidence: id("a"),
                subject: id("s"),
                facet: sym("quality"),
                route: id("r1"),
                environment: id("e1"),
                value: Some(2.0),
                interval: Some((1.0, 3.0)),
                censored: false,
                failed: false,
            },
            FacetSample {
                evidence: id("b"),
                subject: id("s"),
                facet: sym("quality"),
                route: id("r2"),
                environment: id("e1"),
                value: None,
                interval: None,
                censored: true,
                failed: true,
            },
        ];
        let aggregate = aggregate_facets(&samples).unwrap().remove(0);
        assert_eq!(
            (
                aggregate.observed,
                aggregate.censored,
                aggregate.failures,
                aggregate.strata.len(),
                aggregate.provenance.len()
            ),
            (1, 1, 1, 2, 2)
        );
    }

    #[test]
    fn only_declared_pairs_are_causal() {
        let left = id("left");
        let right = id("right");
        assert_eq!(
            contrast(left.clone(), right.clone(), 1.0, 2.0, None)
                .unwrap()
                .kind,
            ContrastKind::Descriptive
        );
        let paired = PairedTreatment {
            left_treatment: left.clone(),
            right_treatment: right.clone(),
            pair_keys: BTreeSet::from([id("pair")]),
        };
        assert_eq!(
            contrast(left, right, 1.0, 2.0, Some(&paired)).unwrap().kind,
            ContrastKind::Causal
        );
    }

    #[test]
    fn spend_and_zero_accepted_economics_are_conservative() {
        let events = vec![
            AttributedResourceEvent {
                evidence: id("candidate"),
                subject: Some(id("s")),
                route: Some(id("r")),
                resource: sym("tokens"),
                amount: 10,
                cause: ResourceCause::CandidateRoute,
            },
            AttributedResourceEvent {
                evidence: id("observer"),
                subject: None,
                route: None,
                resource: sym("tokens"),
                amount: 4,
                cause: ResourceCause::Observer,
            },
        ];
        let summary = summarize_spend(&events);
        let economics = cost_per_accepted(&summary, 0);
        assert_eq!((summary.total, summary.comparable), (14, 10));
        assert_eq!(economics.cost_per_accepted, None);
        assert!(economics.verdict_censored);
    }

    #[test]
    fn missing_cell_specialist_and_cheaper_equivalent_have_conservative_results() {
        let missing = decide(
            spec(),
            vec![
                row("a", Some(0.9), Some(5.0), 2),
                row("b", None, Some(4.0), 2),
            ],
        )
        .unwrap();
        assert_eq!(
            missing
                .subjects
                .iter()
                .find(|r| r.subject == id("b"))
                .unwrap()
                .verdict,
            Verdict::InsufficientEvidence
        );
        let specialist = decide(
            spec(),
            vec![
                row("fast", Some(0.8), Some(2.0), 2),
                row("specialist", Some(0.95), Some(80.0), 2),
            ],
        )
        .unwrap();
        assert!(
            specialist
                .subjects
                .iter()
                .any(|r| r.subject == id("specialist") && r.verdict == Verdict::Incomparable)
        );
        let cheaper = decide(
            spec(),
            vec![
                row("cheap", Some(0.9), Some(5.0), 2),
                row("dear", Some(0.9), Some(10.0), 2),
            ],
        )
        .unwrap();
        assert!(cheaper.edges.iter().any(|edge| edge.left == id("cheap")
            && edge.right == id("dear")
            && edge.relation == Relation::Dominates));
    }

    #[test]
    fn epoch_floor_selection_admission_and_views_are_stable() {
        let specification = spec();
        assert!(!negative_projection_allowed(
            &specification,
            &row("a", Some(1.0), Some(1.0), 1).epochs
        ));
        let evidence = vec![
            row("a", Some(1.0), Some(1.0), 2),
            row("b", Some(0.5), Some(2.0), 2),
        ];
        let report = decide(specification, evidence.clone()).unwrap();
        let selection = select(
            &report,
            &evidence,
            StalenessInputs {
                subject_snapshot: id("snapshot"),
                evidence_root: id("root"),
                policy: id("spec"),
            },
        )
        .unwrap();
        assert_eq!(selection.subjects, vec![id("a")]);
        assert_eq!(report.render_sim(), report.render_sim());
        assert_eq!(report.render_markdown(), report.render_markdown());
        assert_eq!(report.render_csv(), report.render_csv());
        assert_eq!(report.render_table(), report.render_table());
        let mut inadmissible = evidence;
        inadmissible[0].bootstrap = true;
        assert_eq!(
            select(
                &report,
                &inadmissible,
                StalenessInputs {
                    subject_snapshot: id("snapshot"),
                    evidence_root: id("root"),
                    policy: id("spec")
                }
            ),
            Err(DecisionError::InadmissibleSelection)
        );
    }
}
