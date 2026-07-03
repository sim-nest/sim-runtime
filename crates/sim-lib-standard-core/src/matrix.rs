//! Shared language conformance matrix data structures.

use indexmap::IndexMap;
use sim_kernel::{Cx, Error, Expr, Result, Symbol, Value};

use crate::{
    ConformanceOutcome, LanguageProfile, matrix_claims::publish_matrix_cell_claim,
    standard_test_capability,
};

/// Expected outcome for a source-level conformance case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceExpectation {
    /// The source lowers to the described shared expression form.
    LowersTo(String),
    /// The source is an explicit known gap with a machine-readable code.
    ExpectedGap {
        /// Gap code.
        code: Symbol,
        /// Human-readable reason.
        reason: String,
    },
}

/// Observation produced by a language-specific source-case runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceObservation {
    /// Source lowered to the displayed shared form.
    LowersTo(String),
    /// Source is a declared gap with a machine-readable code and reason.
    Gap {
        /// Gap code.
        code: Symbol,
        /// Human-readable reason.
        reason: String,
    },
}

/// One source-language conformance case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceConformanceCase {
    /// Stable symbol identifying this case.
    pub symbol: Symbol,
    /// Organ exercised by this case.
    pub organ: Symbol,
    /// Source filename or display name.
    pub source_name: String,
    /// Source text.
    pub source: String,
    /// Expected result.
    pub expectation: SourceExpectation,
    /// Fidelity badge affected by this case, if any.
    pub affects_badge: Option<Symbol>,
}

/// Codec-faithful source case that decodes to the shared `Expr` graph.
///
/// The case records source text plus the canonical display expected from the
/// decoded expression. A missing expected display means successful decoding is
/// enough; a language-specific decoder returns `Ok(None)` for an explicit gap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprRoundTripCase {
    /// Stable symbol identifying this case.
    pub symbol: Symbol,
    /// Language exercised by this case.
    pub language: Symbol,
    /// Source text.
    pub source: String,
    /// Expected canonical display of the decoded expression.
    pub expected_display: Option<String>,
    /// Fidelity badge affected by this case, if any.
    pub affects_badge: Option<Symbol>,
}

/// Observation produced by running an [`ExprRoundTripCase`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprRoundTripObservation {
    /// Decoded and matched the expected display, or no display was required.
    RoundTripped(String),
    /// Decoded but did not match the expected display.
    Mismatch {
        /// Expected expression display.
        expected: String,
        /// Actual expression display.
        got: String,
    },
    /// Codec returned a diagnostic code.
    Diagnostic(Symbol),
    /// Known gap; decode was not attempted.
    Gap(Symbol),
}

impl ExprRoundTripCase {
    /// Runs this case using `decode_fn` to decode source into an expression.
    pub fn run_expr_round_trip(
        &self,
        cx: &mut Cx,
        decode_fn: impl Fn(&mut Cx, &str) -> Result<Option<Expr>>,
    ) -> ExprRoundTripObservation {
        match decode_fn(cx, &self.source) {
            Err(err) => ExprRoundTripObservation::Diagnostic(Symbol::qualified(
                "codec",
                diagnostic_slug(&err),
            )),
            Ok(None) => ExprRoundTripObservation::Gap(Symbol::qualified("codec", "declared-gap")),
            Ok(Some(expr)) => {
                let got = expr_display(&expr);
                match &self.expected_display {
                    None => ExprRoundTripObservation::RoundTripped(got),
                    Some(expected) if expected == &got => {
                        ExprRoundTripObservation::RoundTripped(got)
                    }
                    Some(expected) => ExprRoundTripObservation::Mismatch {
                        expected: expected.clone(),
                        got,
                    },
                }
            }
        }
    }

    /// Runs this case using `decode_fn` to decode source into an expression.
    pub fn run(
        &self,
        cx: &mut Cx,
        decode_fn: impl Fn(&mut Cx, &str) -> Result<Option<Expr>>,
    ) -> ExprRoundTripObservation {
        self.run_expr_round_trip(cx, decode_fn)
    }
}

/// A single language surface registered in the shared conformance matrix.
///
/// The row contains current conformance evidence for one language profile. Each
/// row uses a stable language symbol, owns the profile metadata for that row,
/// and carries only explicit source or expression cases. An empty row is a
/// declared language entry without scored evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageRow {
    /// Language symbol, for example `scheme` or `lua`.
    pub language: Symbol,
    /// Profile supplied by the language crate.
    pub profile: LanguageProfile,
    /// Source cases registered for this language.
    pub cases: Vec<SourceConformanceCase>,
    /// Expression round-trip cases registered for this language.
    pub expr_cases: Vec<ExprRoundTripCase>,
}

impl LanguageRow {
    /// Declares a language row with no source cases.
    pub fn declared_empty(language: Symbol, profile: LanguageProfile) -> Self {
        Self {
            language,
            profile,
            cases: Vec::new(),
            expr_cases: Vec::new(),
        }
    }

    /// Returns whether this row currently has no cases.
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty() && self.expr_cases.is_empty()
    }

    /// Replaces expression round-trip cases for this row.
    pub fn with_expr_cases(mut self, expr_cases: Vec<ExprRoundTripCase>) -> Self {
        self.expr_cases = expr_cases;
        self
    }
}

/// Builder for [`LanguageRow`] values.
#[derive(Clone, Debug)]
pub struct LanguageRowBuilder {
    language: Symbol,
    profile: LanguageProfile,
    cases: Vec<SourceConformanceCase>,
    expr_cases: Vec<ExprRoundTripCase>,
}

impl LanguageRowBuilder {
    /// Starts a row builder for `language` and `profile`.
    pub fn new(language: Symbol, profile: LanguageProfile) -> Self {
        Self {
            language,
            profile,
            cases: Vec::new(),
            expr_cases: Vec::new(),
        }
    }

    /// Appends one source case.
    pub fn with_case(mut self, case: SourceConformanceCase) -> Self {
        self.cases.push(case);
        self
    }

    /// Appends source cases from an iterator.
    pub fn with_cases<I>(mut self, cases: I) -> Self
    where
        I: IntoIterator<Item = SourceConformanceCase>,
    {
        self.cases.extend(cases);
        self
    }

    /// Appends expression round-trip cases from an iterator.
    pub fn with_expr_cases<I>(mut self, cases: I) -> Self
    where
        I: IntoIterator<Item = ExprRoundTripCase>,
    {
        self.expr_cases.extend(cases);
        self
    }

    /// Builds the row.
    pub fn build(self) -> LanguageRow {
        LanguageRow {
            language: self.language,
            profile: self.profile,
            cases: self.cases,
            expr_cases: self.expr_cases,
        }
    }
}

/// Outcome for a single language/case cell in a matrix run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixCellResult {
    /// Language symbol for this row.
    pub language: Symbol,
    /// Profile symbol for this row.
    pub profile: Symbol,
    /// Organ exercised by this case.
    pub organ: Symbol,
    /// Stable case symbol.
    pub case_symbol: Symbol,
    /// Compared conformance outcome.
    pub outcome: ConformanceOutcome,
}

/// Accumulated results for one matrix run.
///
/// The report is evidence produced by a runner invocation. Gaps remain visible
/// as cells, while fidelity counts only pass and fail cells so declared gaps do
/// not inflate or reduce the score.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixRunReport {
    /// Matrix cells produced by the run.
    pub cells: Vec<MatrixCellResult>,
}

impl MatrixRunReport {
    /// Number of passing cells.
    pub fn pass_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| cell.outcome.is_pass())
            .count()
    }

    /// Number of declared gap cells.
    pub fn gap_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| cell.outcome.is_gap())
            .count()
    }

    /// Number of failing cells.
    pub fn fail_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| cell.outcome.is_fail())
            .count()
    }

    /// Fidelity for one language: passes divided by passes plus failures,
    /// ignoring declared gaps. Returns `None` when no pass-or-fail cells exist.
    pub fn language_fidelity(&self, language: &Symbol) -> Option<f32> {
        let pass = self
            .cells
            .iter()
            .filter(|cell| &cell.language == language && cell.outcome.is_pass())
            .count();
        let fail = self
            .cells
            .iter()
            .filter(|cell| &cell.language == language && cell.outcome.is_fail())
            .count();
        if pass + fail == 0 {
            None
        } else {
            Some(pass as f32 / (pass + fail) as f32)
        }
    }

    /// Produces Card fields for one language's browseable conformance surface.
    ///
    /// These fields answer how much of a language profile is backed by current
    /// matrix evidence for agents and humans browsing the Card.
    pub fn conformance_card_fields(
        &self,
        cx: &mut Cx,
        language: &Symbol,
    ) -> Result<Vec<(Symbol, Value)>> {
        let pass = self.language_outcome_count(language, ConformanceOutcome::is_pass);
        let gap = self.language_outcome_count(language, ConformanceOutcome::is_gap);
        let fail = self.language_outcome_count(language, ConformanceOutcome::is_fail);
        let fidelity = self
            .language_fidelity(language)
            .map(|value| format!("{:.0}%", value * 100.0))
            .unwrap_or_else(|| "unscored".to_owned());
        conformance_card_fields(cx, pass, gap, fail, fidelity)
    }

    /// Produces zero-count conformance Card fields with unscored fidelity.
    pub fn unscored_conformance_card_fields(cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        conformance_card_fields(cx, 0, 0, 0, "unscored".to_owned())
    }

    /// Writes one evidence claim per cell into the claim store.
    pub fn publish_claims(&self, cx: &mut Cx) -> Result<()> {
        cx.require(&standard_test_capability())?;
        for cell in &self.cells {
            publish_matrix_cell_claim(cx, cell)?;
        }
        Ok(())
    }

    fn language_outcome_count(
        &self,
        language: &Symbol,
        matches: impl Fn(&ConformanceOutcome) -> bool,
    ) -> usize {
        self.cells
            .iter()
            .filter(|cell| &cell.language == language && matches(&cell.outcome))
            .count()
    }
}

/// Runs language rows through caller-supplied source-case runners.
///
/// The runner compares the row's expected source outcomes with observations
/// from the caller. It does not depend on a concrete language codec; each
/// language crate supplies its own execution closure and publishes the report
/// when evidence claims are needed.
pub struct MatrixRunner;

impl MatrixRunner {
    /// Runs a single language row, using `run_case` to execute each source case.
    pub fn run_row<F>(cx: &mut Cx, row: &LanguageRow, run_case: F) -> MatrixRunReport
    where
        F: Fn(&mut Cx, &SourceConformanceCase) -> Result<SourceObservation>,
    {
        let mut cells = Vec::with_capacity(row.cases.len());
        for case in &row.cases {
            let outcome = match run_case(cx, case) {
                Ok(observation) => compare_source_observation(case, observation),
                Err(err) => ConformanceOutcome::fail_with(err.to_string()),
            };
            cells.push(MatrixCellResult {
                language: row.language.clone(),
                profile: row.profile.symbol.clone(),
                organ: case.organ.clone(),
                case_symbol: case.symbol.clone(),
                outcome,
            });
        }
        MatrixRunReport { cells }
    }
}

/// Compares a source observation against its expected result.
pub fn compare_source_observation(
    case: &SourceConformanceCase,
    observation: SourceObservation,
) -> ConformanceOutcome {
    match (&case.expectation, observation) {
        (SourceExpectation::LowersTo(expected), SourceObservation::LowersTo(got)) => {
            if expected == &got {
                ConformanceOutcome::pass()
            } else {
                ConformanceOutcome::fail(format!("expected {expected}, got {got}"))
            }
        }
        (
            SourceExpectation::ExpectedGap { code, reason },
            SourceObservation::Gap {
                code: got,
                reason: got_reason,
            },
        ) => {
            if code == &got {
                ConformanceOutcome::gap(reason.clone())
            } else {
                ConformanceOutcome::fail(format!(
                    "expected gap {code}, got gap {got}: {got_reason}"
                ))
            }
        }
        (SourceExpectation::ExpectedGap { code, .. }, SourceObservation::LowersTo(got)) => {
            ConformanceOutcome::fail(format!("expected gap {code}, got {got}"))
        }
        (SourceExpectation::LowersTo(expected), SourceObservation::Gap { code, reason }) => {
            ConformanceOutcome::fail(format!("expected {expected}, got gap {code}: {reason}"))
        }
    }
}

/// Shared conformance matrix keyed by language symbol.
///
/// Rows preserve registration order and are unique by language symbol. The
/// matrix owns row metadata and case definitions only; execution lives in
/// [`MatrixRunner`] and language-specific runners.
#[derive(Default)]
pub struct ConformanceMatrix {
    rows: IndexMap<Symbol, LanguageRow>,
}

impl ConformanceMatrix {
    /// Creates an empty matrix.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a language row.
    ///
    /// # Panics
    ///
    /// Panics when the language symbol is already registered.
    pub fn register(&mut self, row: LanguageRow) {
        let language = row.language.clone();
        assert!(
            self.rows.insert(language.clone(), row).is_none(),
            "language already registered in matrix: {language}",
        );
    }

    /// Number of registered languages.
    pub fn language_count(&self) -> usize {
        self.rows.len()
    }

    /// Returns the row for `language`, if registered.
    pub fn row(&self, language: &Symbol) -> Option<&LanguageRow> {
        self.rows.get(language)
    }

    /// Iterates rows in registration order.
    pub fn iter_rows(&self) -> impl Iterator<Item = &LanguageRow> {
        self.rows.values()
    }

    /// Total source cases across all registered languages.
    pub fn total_cases(&self) -> usize {
        self.rows.values().map(|row| row.cases.len()).sum()
    }

    /// Total expression round-trip cases across all registered languages.
    pub fn total_expr_cases(&self) -> usize {
        self.rows.values().map(|row| row.expr_cases.len()).sum()
    }
}

fn expr_display(expr: &Expr) -> String {
    format!("Expr::{expr:?}")
}

fn diagnostic_slug(err: &Error) -> &'static str {
    if err.to_string().to_ascii_lowercase().contains("unsupported") {
        "unsupported"
    } else {
        "error"
    }
}

fn conformance_card_fields(
    cx: &mut Cx,
    pass: usize,
    gap: usize,
    fail: usize,
    fidelity: String,
) -> Result<Vec<(Symbol, Value)>> {
    Ok(vec![
        (conformance_field("pass"), count_value(cx, pass)?),
        (conformance_field("gap"), count_value(cx, gap)?),
        (conformance_field("fail"), count_value(cx, fail)?),
        (
            conformance_field("fidelity"),
            cx.factory().string(fidelity)?,
        ),
    ])
}

fn conformance_field(name: &str) -> Symbol {
    Symbol::new(format!("conformance.{name}"))
}

fn count_value(cx: &mut Cx, count: usize) -> Result<Value> {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "u64"), count.to_string())
}
