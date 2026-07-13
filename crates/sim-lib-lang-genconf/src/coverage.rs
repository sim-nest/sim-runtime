//! Measured coverage reports for generated expression conformance.

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{Cx, Expr, ReadPolicy, Symbol};
use sim_lib_standard_core::{
    ExprRoundTripCase, ExprRoundTripObservation, LanguageProfile, LanguageRowBuilder, MatrixRunner,
    SourceObservation,
};

use crate::property::{check_round_trip, generated_expr_cases};
use crate::space::ExprSpace;

/// A reproducible measurement of generated expression round-trip coverage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCoverageReport {
    /// Language row measured by this report.
    pub language: Symbol,
    /// Number of generated cases sampled.
    pub sampled: usize,
    /// Number of sampled cases that round-tripped.
    pub round_tripped: usize,
    /// Number of sampled cases that decoded to a different expression.
    pub mismatched: usize,
    /// Number of sampled cases that produced diagnostics or declared gaps.
    pub diagnostics: usize,
    /// Maximum expression depth used by the space.
    pub max_depth: usize,
    /// Seed corpus used as the landmark gate for this measurement.
    pub seed: Vec<Expr>,
    /// Whether every seed landmark round-tripped before coverage reporting.
    pub landmark_reproduced: bool,
    /// Seed expressions that did not round-trip before measurement.
    pub unmet_landmarks: Vec<Expr>,
}

impl GeneratedCoverageReport {
    /// Returns true when every seed landmark round-tripped.
    pub fn landmark_reproduced(&self) -> bool {
        self.landmark_reproduced
    }

    /// Coverage ratio, `round_tripped / sampled`.
    ///
    /// Returns `None` when seed landmark reproduction did not pass or the run
    /// sampled no generated cases.
    pub fn coverage(&self) -> Option<f32> {
        if !self.landmark_reproduced() || self.sampled == 0 {
            return None;
        }
        Some(self.round_tripped as f32 / self.sampled as f32)
    }

    /// Coverage percentage, derived from [`GeneratedCoverageReport::coverage`].
    pub fn coverage_percent(&self) -> Option<f32> {
        self.coverage().map(|ratio| ratio * 100.0)
    }
}

/// Runs generated expression cases for one language and codec.
///
/// The generated cases are attached to a standard language row and the row is
/// passed through [`MatrixRunner`]. Expression observations are counted through
/// the existing [`ExprRoundTripCase`] path.
pub fn run_generated_row(
    cx: &mut Cx,
    language: &Symbol,
    codec: &Symbol,
    space: &ExprSpace,
    budget: usize,
) -> GeneratedCoverageReport {
    let seed = space.seed_corpus();
    let unmet_landmarks = unmet_landmarks(cx, codec, &seed);
    let generated_cases = generated_expr_cases(cx, language, codec, space, budget);
    let row = LanguageRowBuilder::new(
        language.clone(),
        LanguageProfile::new(Symbol::qualified(
            "lang/generated",
            language.as_qualified_str().to_owned(),
        )),
    )
    .with_expr_cases(generated_cases)
    .build();
    let matrix_report = MatrixRunner::run_row(cx, &row, |_cx, _case| {
        Ok(SourceObservation::LowersTo(String::new()))
    });
    debug_assert!(matrix_report.cells.is_empty());

    let mut round_tripped = 0;
    let mut mismatched = 0;
    let mut diagnostics = 0;
    for case in &row.expr_cases {
        match run_expr_case(cx, codec, case) {
            ExprRoundTripObservation::RoundTripped(_) => round_tripped += 1,
            ExprRoundTripObservation::Mismatch { .. } => mismatched += 1,
            ExprRoundTripObservation::Diagnostic(_) | ExprRoundTripObservation::Gap(_) => {
                diagnostics += 1;
            }
        }
    }

    GeneratedCoverageReport {
        language: language.clone(),
        sampled: row.expr_cases.len(),
        round_tripped,
        mismatched,
        diagnostics,
        max_depth: space.max_depth(),
        seed,
        landmark_reproduced: unmet_landmarks.is_empty(),
        unmet_landmarks,
    }
}

fn unmet_landmarks(cx: &mut Cx, codec: &Symbol, seed: &[Expr]) -> Vec<Expr> {
    seed.iter()
        .filter(|expr| {
            !matches!(
                check_round_trip(cx, codec, expr),
                ExprRoundTripObservation::RoundTripped(_)
            )
        })
        .cloned()
        .collect()
}

fn run_expr_case(
    cx: &mut Cx,
    codec: &Symbol,
    case: &ExprRoundTripCase,
) -> ExprRoundTripObservation {
    case.run(cx, |cx, source| {
        decode_with_codec(
            cx,
            codec,
            Input::Text(source.to_owned()),
            ReadPolicy::default(),
        )
        .map(Some)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{DefaultFactory, EagerPolicy, read_eval_capability};
    use sim_lib_lang_scheme::{SchemeCodecLib, scheme_reader_symbol};

    use super::*;

    fn coverage_cx() -> Cx {
        let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        sim_test_support::register_core_classes(&mut cx);
        sim_test_support::register_f64_number_domain(&mut cx);
        cx
    }

    fn register_scheme_codec(cx: &mut Cx) -> Symbol {
        let codec = scheme_reader_symbol();
        let lib = SchemeCodecLib::new(cx.registry_mut().fresh_codec_id());
        cx.load_lib(&lib).unwrap();
        codec
    }

    fn register_lisp_codec(cx: &mut Cx) -> Symbol {
        let codec = Symbol::qualified("codec", "lisp");
        let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
        cx.load_lib(&lib).unwrap();
        codec
    }

    #[test]
    fn generated_source_case_rejects_read_eval_payload() {
        let mut cx = coverage_cx();
        let codec = register_lisp_codec(&mut cx);
        let direct = decode_with_codec(
            &mut cx,
            &codec,
            Input::Text("#. 1".to_owned()),
            ReadPolicy::default(),
        )
        .unwrap_err();
        assert!(
            matches!(direct, sim_kernel::Error::CapabilityDenied { ref capability } if capability == &read_eval_capability()),
            "expected direct read-eval denial, got {direct:?}",
        );
        let case = ExprRoundTripCase {
            symbol: Symbol::qualified("gen/lisp", "read-eval-denied"),
            language: Symbol::new("lisp"),
            source: "#. 1".to_owned(),
            expected_display: None,
            affects_badge: None,
        };

        let observation = run_expr_case(&mut cx, &codec, &case);

        assert!(
            matches!(observation, ExprRoundTripObservation::Diagnostic(_)),
            "read-eval source silently passed: {observation:?}",
        );
    }

    #[test]
    fn scheme_generated_coverage_is_reproducible() {
        let mut first_cx = coverage_cx();
        let mut second_cx = coverage_cx();
        let first_codec = register_scheme_codec(&mut first_cx);
        let second_codec = register_scheme_codec(&mut second_cx);
        let language = Symbol::new("scheme");
        let space = ExprSpace::r7rs_core_space(3);

        let first = run_generated_row(&mut first_cx, &language, &first_codec, &space, 8);
        let second = run_generated_row(&mut second_cx, &language, &second_codec, &space, 8);

        assert_eq!(first, second);
        assert_eq!(first.language, language);
        assert_eq!(first.sampled, 8);
        assert_eq!(first.round_tripped, 0);
        assert_eq!(first.mismatched, 0);
        assert_eq!(first.diagnostics, 8);
        assert_eq!(first.max_depth, 3);
        assert_eq!(first.seed.len(), 5);
        assert_eq!(first.unmet_landmarks.len(), first.seed.len());
        assert_eq!(first.coverage(), None);
    }

    #[test]
    fn coverage_is_none_without_landmark_reproduction() {
        let mut cx = coverage_cx();
        let codec = register_scheme_codec(&mut cx);
        let report = run_generated_row(
            &mut cx,
            &Symbol::new("scheme"),
            &codec,
            &ExprSpace::r7rs_core_space(2),
            4,
        );

        assert!(!report.landmark_reproduced());
        assert_eq!(report.coverage(), None);
        assert!(!report.unmet_landmarks.is_empty());
    }

    #[test]
    fn coverage_ratio_is_round_tripped_over_sampled_after_landmarks() {
        let report = GeneratedCoverageReport {
            language: Symbol::new("scheme"),
            sampled: 4,
            round_tripped: 3,
            mismatched: 1,
            diagnostics: 0,
            max_depth: 2,
            seed: vec![Expr::Bool(true)],
            landmark_reproduced: true,
            unmet_landmarks: Vec::new(),
        };

        assert!(report.landmark_reproduced());
        assert_eq!(report.coverage(), Some(0.75));
        assert_eq!(report.coverage_percent(), Some(75.0));
    }
}
