//! Conformance harness running profile test cases and reporting fidelity.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sim_kernel::{
    Claim, ClaimKind, ClaimPattern, Cx, Datum, DatumStore, OpKey, Ref, Result, Symbol,
    card::{card_kind_predicate, card_tests_predicate},
    standard::standard_evidence_predicate,
};

use crate::{FidelityBadge, LanguageProfile, standard_test_capability};

/// A conformance check: runs a profile against the runtime and reports an outcome.
pub type ConformanceCheck =
    Arc<dyn Fn(&mut Cx, &LanguageProfile) -> Result<ConformanceOutcome> + Send + Sync + 'static>;

/// Status of a conformance outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConformanceStatus {
    /// The check passed.
    Pass,
    /// The check failed.
    Fail,
    /// The case is a declared gap and is excluded from fidelity ratios.
    Gap,
}

/// Result of running one [`ConformanceTestCase`]: pass, fail, or declared gap
/// with optional detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceOutcome {
    /// Whether the check passed.
    pub passed: bool,
    /// Optional failure detail.
    pub detail: Option<String>,
    /// Exact status used by matrix runners and claim publication.
    pub status: ConformanceStatus,
}

impl ConformanceOutcome {
    /// A passing outcome with no detail.
    pub fn pass() -> Self {
        Self {
            passed: true,
            detail: None,
            status: ConformanceStatus::Pass,
        }
    }

    /// A failing outcome carrying `detail`.
    pub fn fail(detail: impl Into<String>) -> Self {
        Self {
            passed: false,
            detail: Some(detail.into()),
            status: ConformanceStatus::Fail,
        }
    }

    /// A failing outcome carrying `detail`.
    pub fn fail_with(detail: impl Into<String>) -> Self {
        Self::fail(detail)
    }

    /// A declared gap outcome carrying a detail string.
    pub fn gap(detail: impl Into<String>) -> Self {
        Self {
            passed: false,
            detail: Some(detail.into()),
            status: ConformanceStatus::Gap,
        }
    }

    /// Returns whether this outcome is a pass.
    pub fn is_pass(&self) -> bool {
        self.status == ConformanceStatus::Pass
    }

    /// Returns whether this outcome is a fail.
    pub fn is_fail(&self) -> bool {
        self.status == ConformanceStatus::Fail
    }

    /// Returns whether this outcome is a declared gap.
    pub fn is_gap(&self) -> bool {
        self.status == ConformanceStatus::Gap
    }

    /// Returns the standard status symbol for this outcome.
    pub fn status_symbol(&self) -> Symbol {
        match self.status {
            ConformanceStatus::Pass => Symbol::qualified("standard/test", "pass"),
            ConformanceStatus::Fail => Symbol::qualified("standard/test", "fail"),
            ConformanceStatus::Gap => Symbol::qualified("standard/test", "gap"),
        }
    }
}

/// One conformance test: its symbol, the organ it covers, an optional badge it
/// affects, and the check closure.
#[derive(Clone)]
pub struct ConformanceTestCase {
    /// Symbol identifying the test.
    pub symbol: Symbol,
    /// Organ the test exercises.
    pub organ: Symbol,
    /// Fidelity badge whose level drops if this test fails, if any.
    pub affected_badge: Option<Symbol>,
    check: ConformanceCheck,
}

impl ConformanceTestCase {
    /// Build a test for `organ` identified by `symbol`, running `check`.
    pub fn new(symbol: Symbol, organ: Symbol, check: ConformanceCheck) -> Self {
        Self {
            symbol,
            organ,
            affected_badge: None,
            check,
        }
    }

    /// Mark this test as affecting `badge`, lowering its level on failure.
    pub fn affecting_badge(mut self, badge: Symbol) -> Self {
        self.affected_badge = Some(badge);
        self
    }

    fn run(&self, cx: &mut Cx, profile: &LanguageProfile) -> Result<ConformanceOutcome> {
        (self.check)(cx, profile)
    }
}

/// Registry of conformance tests grouped by the organ they cover.
#[derive(Default)]
pub struct ConformanceHarness {
    tests: BTreeMap<Symbol, Vec<ConformanceTestCase>>,
}

impl ConformanceHarness {
    /// Create an empty harness.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `test` under its organ.
    pub fn register_test(&mut self, test: ConformanceTestCase) {
        self.tests.entry(test.organ.clone()).or_default().push(test);
    }

    /// Tests registered for `organ`, or an empty slice if none.
    pub fn tests_for_organ(&self, organ: &Symbol) -> &[ConformanceTestCase] {
        self.tests.get(organ).map(Vec::as_slice).unwrap_or_default()
    }

    /// Total number of registered tests across all organs.
    pub fn test_count(&self) -> usize {
        self.tests.values().map(Vec::len).sum()
    }
}

/// Report of running the harness against a profile: per-organ results and the
/// fidelity badges as lowered by any failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandardTestReport {
    /// Symbol of the tested profile.
    pub profile: Symbol,
    /// Per-organ test reports.
    pub organs: Vec<OrganTestReport>,
    /// Fidelity badges after applying test failures.
    pub reported_badges: Vec<FidelityBadge>,
}

impl StandardTestReport {
    /// Whether every organ's tests passed.
    pub fn passed(&self) -> bool {
        self.organs.iter().all(OrganTestReport::passed)
    }

    /// Total number of test results across all organs.
    pub fn result_count(&self) -> usize {
        self.organs.iter().map(|organ| organ.tests.len()).sum()
    }
}

/// Per-organ slice of a [`StandardTestReport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganTestReport {
    /// The organ these results cover.
    pub organ: Symbol,
    /// Per-test reports for this organ.
    pub tests: Vec<ConformanceTestReport>,
}

impl OrganTestReport {
    /// Whether every test for this organ passed.
    pub fn passed(&self) -> bool {
        self.tests.iter().all(|test| test.passed)
    }
}

/// Result of one conformance test, with a reference to its published evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceTestReport {
    /// Symbol of the test.
    pub test: Symbol,
    /// Whether the test passed.
    pub passed: bool,
    /// Optional failure detail.
    pub detail: Option<String>,
    /// Reference to the published test-run evidence.
    pub evidence: Ref,
}

/// Operation key for the standard test operation.
pub fn standard_test_op_key() -> OpKey {
    OpKey::new(Symbol::new("standard"), Symbol::new("test"), 1)
}

/// Datum tag identifying a published test-run record.
pub fn standard_test_run_kind() -> Symbol {
    Symbol::qualified("standard", "test-run")
}

/// Claim predicate relating a subject to a test-run evidence ref.
pub fn standard_test_result_predicate() -> Symbol {
    standard_symbol("test-result")
}

/// Claim predicate relating a test run to its profile.
pub fn standard_test_profile_predicate() -> Symbol {
    standard_symbol("test-profile")
}

/// Claim predicate relating a test run to its organ.
pub fn standard_test_organ_predicate() -> Symbol {
    standard_symbol("test-organ")
}

/// Claim predicate relating a test run to its test case.
pub fn standard_test_case_predicate() -> Symbol {
    standard_symbol("test-case")
}

/// Claim predicate relating a test run to its pass/fail status.
pub fn standard_test_status_predicate() -> Symbol {
    standard_symbol("test-status")
}

/// Claim predicate relating a subject to its reported fidelity badge.
pub fn standard_reported_fidelity_predicate() -> Symbol {
    standard_symbol("reported-fidelity")
}

/// Claim predicate relating a subject to its reported fidelity level.
pub fn standard_reported_fidelity_level_predicate() -> Symbol {
    standard_symbol("reported-fidelity-level")
}

/// Run `harness` against `profile`, gated on [`standard_test_capability`].
///
/// Each test publishes a test-run record and claims; a failed test lowers the
/// level of any badge it affects. Returns a [`StandardTestReport`].
///
/// [`standard_test_capability`]: crate::standard_test_capability
pub fn standard_test_stub(
    cx: &mut Cx,
    harness: &ConformanceHarness,
    profile: &LanguageProfile,
) -> Result<StandardTestReport> {
    cx.require(&standard_test_capability())?;
    let mut organs = Vec::with_capacity(profile.organs.len());
    let mut failed_badges = BTreeMap::<Symbol, Ref>::new();

    for organ in &profile.organs {
        let mut tests = Vec::new();
        for test in harness.tests_for_organ(&organ.organ) {
            let outcome = test.run(cx, profile)?;
            let evidence = publish_test_run(cx, profile, &organ.organ, test, &outcome)?;
            if outcome.is_fail()
                && let Some(badge) = &test.affected_badge
            {
                failed_badges.insert(badge.clone(), evidence.clone());
            }
            tests.push(ConformanceTestReport {
                test: test.symbol.clone(),
                passed: outcome.passed,
                detail: outcome.detail,
                evidence,
            });
        }
        organs.push(OrganTestReport {
            organ: organ.organ.clone(),
            tests,
        });
    }

    let reported_badges = lowered_badges(profile, &failed_badges);
    publish_reported_badges(cx, &reported_badges)?;
    Ok(StandardTestReport {
        profile: profile.symbol.clone(),
        organs,
        reported_badges,
    })
}

fn lowered_badges(
    profile: &LanguageProfile,
    failed_badges: &BTreeMap<Symbol, Ref>,
) -> Vec<FidelityBadge> {
    profile
        .fidelity_badges
        .iter()
        .map(|badge| {
            let mut reported = badge.clone();
            if let Some(evidence) = failed_badges.get(&badge.badge) {
                reported.level = reported.level.saturating_sub(1);
                reported.evidence = evidence.clone();
            }
            reported
        })
        .collect()
}

fn publish_test_run(
    cx: &mut Cx,
    profile: &LanguageProfile,
    organ: &Symbol,
    test: &ConformanceTestCase,
    outcome: &ConformanceOutcome,
) -> Result<Ref> {
    let evidence = test_run_ref(cx, profile, organ, test, outcome)?;
    let status = outcome.status_symbol();
    insert_observed_once(
        cx,
        evidence.clone(),
        card_kind_predicate(),
        Ref::Symbol(standard_test_run_kind()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        card_tests_predicate(),
        Ref::Symbol(test.symbol.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_profile_predicate(),
        Ref::Symbol(profile.symbol.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_organ_predicate(),
        Ref::Symbol(organ.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_case_predicate(),
        Ref::Symbol(test.symbol.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_status_predicate(),
        Ref::Symbol(status),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(profile.symbol.clone()),
        standard_test_result_predicate(),
        evidence.clone(),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(organ.clone()),
        standard_test_result_predicate(),
        evidence.clone(),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(profile.symbol.clone()),
        standard_evidence_predicate(),
        evidence.clone(),
    )?;
    Ok(evidence)
}

fn publish_reported_badges(cx: &mut Cx, badges: &[FidelityBadge]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for badge in badges {
        if !seen.insert((badge.subject.clone(), badge.badge.clone())) {
            continue;
        }
        let evidence = vec![badge.evidence.clone()];
        insert_observed_with_evidence_once(
            cx,
            badge.subject.clone(),
            standard_reported_fidelity_predicate(),
            Ref::Symbol(badge.badge.clone()),
            evidence.clone(),
        )?;
        insert_observed_with_evidence_once(
            cx,
            badge.subject.clone(),
            standard_reported_fidelity_level_predicate(),
            Ref::Symbol(Symbol::qualified(
                "standard/fidelity-level",
                badge.level.to_string(),
            )),
            evidence,
        )?;
    }
    Ok(())
}

fn test_run_ref(
    cx: &mut Cx,
    profile: &LanguageProfile,
    organ: &Symbol,
    test: &ConformanceTestCase,
    outcome: &ConformanceOutcome,
) -> Result<Ref> {
    let mut fields = vec![
        (
            Symbol::new("profile"),
            Datum::Symbol(profile.symbol.clone()),
        ),
        (Symbol::new("organ"), Datum::Symbol(organ.clone())),
        (Symbol::new("test"), Datum::Symbol(test.symbol.clone())),
        (Symbol::new("passed"), Datum::Bool(outcome.passed)),
        (
            Symbol::new("status"),
            Datum::Symbol(outcome.status_symbol()),
        ),
    ];
    if let Some(detail) = &outcome.detail {
        fields.push((Symbol::new("detail"), Datum::String(detail.clone())));
    }
    cx.datum_store_mut()
        .intern(Datum::Node {
            tag: standard_test_run_kind(),
            fields,
        })
        .map(Ref::Content)
}

fn insert_observed_once(cx: &mut Cx, subject: Ref, predicate: Symbol, object: Ref) -> Result<()> {
    insert_observed_with_evidence_once(cx, subject, predicate, object, Vec::new())
}

fn insert_observed_with_evidence_once(
    cx: &mut Cx,
    subject: Ref,
    predicate: Symbol,
    object: Ref,
    evidence: Vec<Ref>,
) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        cx.insert_fact(
            Claim::public(subject, predicate, object)
                .with_kind(ClaimKind::Observed)
                .with_evidence(evidence),
        )?;
    }
    Ok(())
}

fn standard_symbol(name: &str) -> Symbol {
    Symbol::qualified("standard", name.to_owned())
}
