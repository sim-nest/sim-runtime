use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{ContentId, Datum, Symbol};

use super::{
    ConclusionId, Explanation, FactId, FederatedClosure,
    assay_repair::{ProjectionRepairItem, ProjectionRepairSet},
};

const REQUIRED_DELTAS: [&str; 6] = ["D1", "D2", "D3", "D4", "D5", "D6"];

/// Semantic class of one controlled stage-one delta.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlledDeltaClass {
    /// An identical semantic input batch.
    NoSemanticChange,
    /// One owner's private implementation changes.
    PrivateImplementation,
    /// A declared public API changes.
    PublicApi,
    /// Three public heads and their control-plane pins change together.
    PublicHeadsAndPins,
    /// Proof implementation and checker implementation identities change.
    ProofAndCheckerImplementation,
    /// Display or ordering changes without changing semantics.
    PresentationOnly,
}

/// One controlled input delta. It predicts work but never executes proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlledDelta {
    /// Stable assay row (`D1` through `D6`).
    pub id: String,
    /// Semantic class under test.
    pub class: ControlledDeltaClass,
    /// Exact changed semantic facts; empty for semantic no-ops.
    pub changed_facts: BTreeSet<FactId>,
    /// Maximum journal revision movement produced by normalization.
    pub revision_delta: u64,
    /// Whether the prediction emitted carried execution state.
    pub carried_state: bool,
}

impl ControlledDelta {
    /// Constructs a checked controlled delta.
    pub fn new(
        id: impl Into<String>,
        class: ControlledDeltaClass,
        changed_facts: impl IntoIterator<Item = FactId>,
        revision_delta: u64,
        carried_state: bool,
    ) -> Result<Self, AssayError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(AssayError::InvalidDeltaId);
        }
        let changed_facts = changed_facts.into_iter().collect::<BTreeSet<_>>();
        if matches!(
            class,
            ControlledDeltaClass::NoSemanticChange | ControlledDeltaClass::PresentationOnly
        ) && !changed_facts.is_empty()
        {
            return Err(AssayError::SemanticNoOpCarriesChangedFacts(id));
        }
        Ok(Self {
            id,
            class,
            changed_facts,
            revision_delta,
            carried_state,
        })
    }
}

/// One independently authored expected affected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedClosure {
    /// Controlled delta identity.
    pub delta: String,
    /// Declared-conclusion denominator used for every percentage.
    pub denominator: usize,
    /// Maximum affected conclusions admitted by the assay.
    pub max_affected: usize,
    /// Maximum semantic journal revision movement.
    pub max_revision_delta: u64,
    /// Whether carried execution state is allowed.
    pub carried_state: bool,
    /// Exact independently expected affected conclusions.
    pub affected: BTreeSet<ConclusionId>,
}

/// Frozen independently authored stage-one oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedClosureSet {
    source: ContentId,
    id: ContentId,
    rows: BTreeMap<String, ExpectedClosure>,
}

impl ExpectedClosureSet {
    /// Freezes six unique D1-D6 rows under the exact authored-source identity.
    pub fn freeze(
        source: ContentId,
        rows: impl IntoIterator<Item = ExpectedClosure>,
    ) -> Result<Self, AssayError> {
        let mut canonical = BTreeMap::new();
        for row in rows {
            if canonical.insert(row.delta.clone(), row).is_some() {
                return Err(AssayError::DuplicateDelta);
            }
        }
        let actual = canonical.keys().map(String::as_str).collect::<Vec<_>>();
        if actual != REQUIRED_DELTAS {
            return Err(AssayError::IncompleteDeltaSet(actual.join(",")));
        }
        let denominator = canonical["D1"].denominator;
        for row in canonical.values() {
            if row.denominator == 0
                || row.denominator != denominator
                || row.max_affected > row.denominator
                || row.affected.len() > row.max_affected
            {
                return Err(AssayError::InvalidExpectedClosure {
                    delta: row.delta.clone(),
                    reason: ExpectedClosureViolation::InconsistentDenominatorOrCeiling,
                });
            }
        }
        let d1 = &canonical["D1"];
        if !d1.affected.is_empty()
            || d1.max_affected != 0
            || d1.max_revision_delta != 0
            || d1.carried_state
        {
            return Err(AssayError::InvalidExpectedClosure {
                delta: "D1".into(),
                reason: ExpectedClosureViolation::NoChangeContract,
            });
        }
        let d2 = &canonical["D2"];
        if unaffected_basis_points(d2.denominator, d2.affected.len()) < 9_500 {
            return Err(AssayError::InvalidExpectedClosure {
                delta: "D2".into(),
                reason: ExpectedClosureViolation::UnaffectedFloor,
            });
        }
        let d4 = &canonical["D4"];
        if unaffected_basis_points(d4.denominator, d4.affected.len()) < 7_000 {
            return Err(AssayError::InvalidExpectedClosure {
                delta: "D4".into(),
                reason: ExpectedClosureViolation::UnaffectedFloor,
            });
        }
        let d6 = &canonical["D6"];
        if !d6.affected.is_empty()
            || d6.max_affected != 0
            || d6.max_revision_delta > 1
            || d6.carried_state
        {
            return Err(AssayError::InvalidExpectedClosure {
                delta: "D6".into(),
                reason: ExpectedClosureViolation::PresentationOnlyContract,
            });
        }
        let datum = Datum::Node {
            tag: Symbol::qualified("projection", "expected-closure-set-v1"),
            fields: vec![
                (Symbol::new("source"), content_id_datum(&source)),
                (
                    Symbol::new("rows"),
                    Datum::Vector(canonical.values().map(expected_datum).collect()),
                ),
            ],
        };
        let id = datum
            .content_id()
            .map_err(|error| AssayError::Canonical(error.to_string()))?;
        Ok(Self {
            source,
            id,
            rows: canonical,
        })
    }

    /// Returns the exact authored-source identity.
    #[must_use]
    pub fn source(&self) -> &ContentId {
        &self.source
    }

    /// Returns the canonical frozen-oracle identity.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }
}

/// Prediction-only work counters. There is deliberately no proof result field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredictedWork {
    /// Number of planner decisions emitted.
    pub decisions: usize,
    /// Number of semantic changed facts consumed.
    pub changed_facts: usize,
    /// Number of exact explanation paths emitted.
    pub explanations: usize,
    /// Number of semantic journal records predicted.
    pub journal_records: usize,
    /// Number of work items a later planner would start.
    pub planned_starts: usize,
}

/// Complete stage-one prediction for one controlled delta.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredictedClosureReport {
    /// Delta identity.
    pub delta: String,
    /// Exact predicted affected set.
    pub affected: BTreeSet<ConclusionId>,
    /// Exact predicted unaffected set.
    pub unaffected: BTreeSet<ConclusionId>,
    /// Explicit declared-conclusion denominator.
    pub denominator: usize,
    /// Integer basis points of conclusions predicted unaffected.
    pub unaffected_basis_points: u16,
    /// Predicted semantic journal revision movement.
    pub revision_delta: u64,
    /// Whether the prediction emitted carried execution state.
    pub carried_state: bool,
    /// Exact causal explanation paths.
    pub explanations: Vec<Explanation>,
    /// Prediction-only counters.
    pub work: PredictedWork,
}

/// Closed stage-one contracts that can require projection repair.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AssayContract {
    /// A controlled delta used the wrong semantic class.
    DeltaClass,
    /// A changed fact was absent from the sealed fact universe.
    UnknownChangedFact,
    /// The sealed graph disagreed with the declared denominator.
    Denominator,
    /// The prediction exceeded its affected-conclusion ceiling.
    AffectedCeiling,
    /// The prediction exceeded its journal revision ceiling.
    RevisionDelta,
    /// Carried-state emission disagreed with the oracle.
    CarriedState,
    /// D1 did not leave every conclusion unaffected.
    D1UnaffectedFloor,
    /// D2 left less than 95 percent of conclusions unaffected.
    D2UnaffectedFloor,
    /// D4 left less than 70 percent of conclusions unaffected.
    D4UnaffectedFloor,
    /// D6 emitted carried state.
    D6CarriedState,
    /// A semantic no-op predicted journal or execution work.
    SemanticNoOpWork,
}

/// One assay row either matches its frozen oracle or enters bounded repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssayOutcome {
    /// Exact expected closure and every row contract passed.
    Passed(PredictedClosureReport),
    /// The report is retained with the canonical bounded repair set.
    Repair {
        /// Failed prediction.
        report: PredictedClosureReport,
        /// Exact locations to repair.
        repairs: ProjectionRepairSet,
    },
}

/// Qualification token proving all six stage-one deltas passed together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageOneQualification {
    /// Frozen oracle identity.
    pub oracle: ContentId,
    /// Six passing reports in D1-D6 order.
    pub reports: Vec<PredictedClosureReport>,
}

/// Pure prediction-only assay over a sealed federated closure.
pub struct PredictedClosureAssay<'a> {
    closure: &'a FederatedClosure,
}

impl<'a> PredictedClosureAssay<'a> {
    /// Binds the sealed projection closure under test.
    #[must_use]
    pub const fn new(closure: &'a FederatedClosure) -> Self {
        Self { closure }
    }

    /// Predicts one row and compares it with the independently frozen oracle.
    pub fn predict(
        &self,
        delta: &ControlledDelta,
        expected: &ExpectedClosureSet,
    ) -> Result<AssayOutcome, AssayError> {
        let oracle = expected
            .rows
            .get(&delta.id)
            .ok_or_else(|| AssayError::UnknownDelta(delta.id.clone()))?;
        let all = self.closure.conclusions().cloned().collect::<BTreeSet<_>>();
        let affected = self
            .closure
            .affected(delta.changed_facts.iter().cloned())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let unaffected = all.difference(&affected).cloned().collect::<BTreeSet<_>>();
        let explanations = affected
            .iter()
            .flat_map(|conclusion| {
                delta
                    .changed_facts
                    .iter()
                    .filter_map(move |fact| self.closure.explain(conclusion, fact).ok())
            })
            .collect::<Vec<_>>();
        let semantic = !delta.changed_facts.is_empty();
        let work = PredictedWork {
            decisions: affected.len(),
            changed_facts: delta.changed_facts.len(),
            explanations: explanations.len(),
            journal_records: usize::from(semantic) * delta.changed_facts.len(),
            planned_starts: affected.len(),
        };
        let basis_points = unaffected_basis_points(all.len(), affected.len());
        let report = PredictedClosureReport {
            delta: delta.id.clone(),
            affected: affected.clone(),
            unaffected,
            denominator: all.len(),
            unaffected_basis_points: basis_points,
            revision_delta: delta.revision_delta,
            carried_state: delta.carried_state,
            explanations,
            work,
        };
        let mut repairs = BTreeSet::new();
        if expected_delta_class(&delta.id) != Some(delta.class) {
            repairs.insert(ProjectionRepairItem::Contract(AssayContract::DeltaClass));
        }
        if delta
            .changed_facts
            .iter()
            .any(|fact| !self.closure.contains_fact(fact))
        {
            repairs.insert(ProjectionRepairItem::Contract(
                AssayContract::UnknownChangedFact,
            ));
        }
        if oracle.denominator != all.len() {
            repairs.insert(ProjectionRepairItem::Contract(AssayContract::Denominator));
        }
        for conclusion in affected.difference(&oracle.affected) {
            for fact in &delta.changed_facts {
                if self.closure.depends_on(conclusion, fact) {
                    repairs.insert(ProjectionRepairItem::GraphEdge {
                        conclusion: conclusion.clone(),
                        fact: fact.clone(),
                    });
                }
            }
        }
        for conclusion in oracle.affected.difference(&affected) {
            repairs.insert(ProjectionRepairItem::Declaration(conclusion.clone()));
        }
        if affected.len() > oracle.max_affected {
            repairs.insert(ProjectionRepairItem::Contract(
                AssayContract::AffectedCeiling,
            ));
        }
        if delta.revision_delta > oracle.max_revision_delta {
            repairs.insert(ProjectionRepairItem::Contract(AssayContract::RevisionDelta));
        }
        if delta.carried_state != oracle.carried_state {
            repairs.insert(ProjectionRepairItem::Contract(AssayContract::CarriedState));
        }
        match delta.id.as_str() {
            "D1" if report.unaffected_basis_points != 10_000 => {
                repairs.insert(ProjectionRepairItem::Contract(
                    AssayContract::D1UnaffectedFloor,
                ));
            }
            "D2" if report.unaffected_basis_points < 9_500 => {
                repairs.insert(ProjectionRepairItem::Contract(
                    AssayContract::D2UnaffectedFloor,
                ));
            }
            "D4" if report.unaffected_basis_points < 7_000 => {
                repairs.insert(ProjectionRepairItem::Contract(
                    AssayContract::D4UnaffectedFloor,
                ));
            }
            "D6" if report.carried_state => {
                repairs.insert(ProjectionRepairItem::Contract(
                    AssayContract::D6CarriedState,
                ));
            }
            _ => {}
        }
        if !semantic && (report.work.planned_starts != 0 || report.work.journal_records != 0) {
            repairs.insert(ProjectionRepairItem::Contract(
                AssayContract::SemanticNoOpWork,
            ));
        }
        if repairs.is_empty() {
            Ok(AssayOutcome::Passed(report))
        } else {
            Ok(AssayOutcome::Repair {
                report,
                repairs: ProjectionRepairSet::new(&delta.id, repairs)?,
            })
        }
    }

    /// Qualifies exactly D1-D6 together; any repair prevents the token.
    pub fn qualify(
        &self,
        deltas: &[ControlledDelta],
        expected: &ExpectedClosureSet,
    ) -> Result<StageOneQualification, AssayError> {
        let actual = deltas
            .iter()
            .map(|delta| delta.id.as_str())
            .collect::<Vec<_>>();
        if actual != REQUIRED_DELTAS {
            return Err(AssayError::IncompleteDeltaSet(actual.join(",")));
        }
        let mut reports = Vec::with_capacity(REQUIRED_DELTAS.len());
        for delta in deltas {
            match self.predict(delta, expected)? {
                AssayOutcome::Passed(report) => reports.push(report),
                AssayOutcome::Repair { repairs, .. } => {
                    return Err(AssayError::RepairRequired(repairs));
                }
            }
        }
        Ok(StageOneQualification {
            oracle: expected.id.clone(),
            reports,
        })
    }
}

/// Invalid oracle, delta, graph, or failed stage-one qualification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssayError {
    /// Delta id is empty.
    InvalidDeltaId,
    /// Two expected rows name one delta.
    DuplicateDelta,
    /// Exact D1-D6 row set or ordering is absent.
    IncompleteDeltaSet(String),
    /// Requested delta is absent from the frozen oracle.
    UnknownDelta(String),
    /// An expected row contradicts the stage-one measurement contract.
    InvalidExpectedClosure {
        /// Invalid controlled delta.
        delta: String,
        /// Stable reason code.
        reason: ExpectedClosureViolation,
    },
    /// A semantic no-op declared changed semantic facts.
    SemanticNoOpCarriesChangedFacts(String),
    /// Canonical evidence construction failed.
    Canonical(String),
    /// At least one exact prediction requires repair.
    RepairRequired(ProjectionRepairSet),
}

/// Structural reason an independently authored oracle cannot be frozen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedClosureViolation {
    /// Denominators differ, are zero, or conflict with affected ceilings.
    InconsistentDenominatorOrCeiling,
    /// D1 does not describe a complete semantic no-op.
    NoChangeContract,
    /// D2 or D4 violates its required unaffected percentage.
    UnaffectedFloor,
    /// D6 predicts semantic work, excessive revision, or carried state.
    PresentationOnlyContract,
}

impl std::fmt::Display for AssayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AssayError {}

fn content_id_datum(id: &ContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("core", "content-id-v1"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(id.algorithm.clone()),
            ),
            (Symbol::new("digest"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}

fn expected_delta_class(delta: &str) -> Option<ControlledDeltaClass> {
    match delta {
        "D1" => Some(ControlledDeltaClass::NoSemanticChange),
        "D2" => Some(ControlledDeltaClass::PrivateImplementation),
        "D3" => Some(ControlledDeltaClass::PublicApi),
        "D4" => Some(ControlledDeltaClass::PublicHeadsAndPins),
        "D5" => Some(ControlledDeltaClass::ProofAndCheckerImplementation),
        "D6" => Some(ControlledDeltaClass::PresentationOnly),
        _ => None,
    }
}

fn unaffected_basis_points(denominator: usize, affected: usize) -> u16 {
    if denominator == 0 || affected > denominator {
        return 0;
    }
    u16::try_from((denominator - affected).saturating_mul(10_000) / denominator).unwrap_or(10_000)
}

fn expected_datum(row: &ExpectedClosure) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("projection", "expected-closure-v1"),
        fields: vec![
            (Symbol::new("delta"), Datum::String(row.delta.clone())),
            (
                Symbol::new("denominator"),
                Datum::String(row.denominator.to_string()),
            ),
            (
                Symbol::new("max-affected"),
                Datum::String(row.max_affected.to_string()),
            ),
            (
                Symbol::new("max-revision-delta"),
                Datum::String(row.max_revision_delta.to_string()),
            ),
            (Symbol::new("carried-state"), Datum::Bool(row.carried_state)),
            (
                Symbol::new("affected"),
                Datum::Vector(
                    row.affected
                        .iter()
                        .map(|id| Datum::String(id.as_str().to_owned()))
                        .collect(),
                ),
            ),
        ],
    }
}
