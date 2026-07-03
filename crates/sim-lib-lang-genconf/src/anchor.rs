//! Curated landmark anchors for generated coverage reports.

use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Expr, SourceId, Symbol};
use sim_lib_lang_cl::cl_lite_source_cases;
use sim_lib_lang_scheme::r7rs_small_expr_cases;

use crate::GeneratedCoverageReport;

/// A curated, citation-traceable set of landmark expressions for one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandmarkCorpus {
    /// Language row anchored by this corpus.
    pub language: Symbol,
    /// Citation tag for the curated standard source.
    pub citation: Symbol,
    /// Highest fidelity level that curated evidence supports for this row.
    pub curated_fidelity_level: u8,
    /// Expressions that generated coverage must reproduce before publication.
    pub landmarks: Vec<Expr>,
}

impl LandmarkCorpus {
    /// Builds a curated landmark corpus.
    pub fn new(
        language: Symbol,
        citation: Symbol,
        curated_fidelity_level: u8,
        landmarks: Vec<Expr>,
    ) -> Self {
        Self {
            language,
            citation,
            curated_fidelity_level: curated_fidelity_level.min(5),
            landmarks,
        }
    }

    /// Reconciles a generated report against this curated anchor.
    ///
    /// Coverage is publishable only when every curated landmark is present in
    /// the report seed and none appear in the unmet landmark list.
    pub fn reconcile(&self, report: &GeneratedCoverageReport) -> CoverageVerdict {
        if report.language != self.language {
            return self.unanchored("report language does not match landmark corpus");
        }
        if let Some(missing) = self.missing_from_seed(report).first() {
            return self.unanchored(format!("curated landmark absent from run: {missing:?}"));
        }
        if let Some(unmet) = self.unmet_in_report(report).first() {
            return self.unanchored(format!(
                "curated landmark not reproduced by generator: {unmet:?}"
            ));
        }
        let Some(coverage_percent) = report.coverage_percent() else {
            return self.unanchored("generated coverage has no publishable percentage");
        };

        let measured_level = coverage_level(coverage_percent);
        CoverageVerdict::Anchored {
            citation: self.citation.clone(),
            coverage_percent,
            curated_fidelity_level: self.curated_fidelity_level,
            claim_level: measured_level.min(self.curated_fidelity_level),
        }
    }

    fn missing_from_seed(&self, report: &GeneratedCoverageReport) -> Vec<Expr> {
        self.landmarks
            .iter()
            .filter(|landmark| !report.seed.iter().any(|seed| seed.canonical_eq(landmark)))
            .cloned()
            .collect()
    }

    fn unmet_in_report(&self, report: &GeneratedCoverageReport) -> Vec<Expr> {
        self.landmarks
            .iter()
            .filter(|landmark| {
                report
                    .unmet_landmarks
                    .iter()
                    .any(|unmet| unmet.canonical_eq(landmark))
            })
            .cloned()
            .collect()
    }

    fn unanchored(&self, reason: impl Into<String>) -> CoverageVerdict {
        CoverageVerdict::Unanchored {
            citation: self.citation.clone(),
            reason: reason.into(),
        }
    }
}

/// Reconciled coverage publication verdict.
#[derive(Clone, Debug, PartialEq)]
pub enum CoverageVerdict {
    /// Landmarks were reproduced; coverage is supplementary evidence.
    Anchored {
        /// Citation tag for the curated standard source.
        citation: Symbol,
        /// Measured coverage percentage from the generated report.
        coverage_percent: f32,
        /// Curated fidelity level for the row.
        curated_fidelity_level: u8,
        /// Published claim level, clamped to curated fidelity.
        claim_level: u8,
    },
    /// At least one landmark was not reproduced; no coverage number is public.
    Unanchored {
        /// Citation tag for the curated standard source.
        citation: Symbol,
        /// Reason coverage cannot be published.
        reason: String,
    },
}

/// Builds the Scheme R7RS-small landmark corpus from curated expression cases.
pub fn r7rs_small_landmark_corpus() -> LandmarkCorpus {
    LandmarkCorpus::new(
        Symbol::new("scheme"),
        Symbol::new("r7rs-small"),
        1,
        r7rs_small_expr_cases()
            .into_iter()
            .filter_map(|case| {
                case.expected_display
                    .as_deref()
                    .and_then(expr_from_expected_display)
            })
            .collect(),
    )
}

/// Builds the Common Lisp lite landmark corpus from curated source cases.
pub fn common_lisp_lite_landmark_corpus() -> LandmarkCorpus {
    LandmarkCorpus::new(
        Symbol::new("common-lisp"),
        Symbol::new("cl-hyperspec"),
        1,
        cl_lite_source_cases()
            .into_iter()
            .filter_map(|case| parse_cl_landmark(&case.source))
            .collect(),
    )
}

fn parse_cl_landmark(source: &str) -> Option<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    sim_lib_lang_cl::parse_cl_lite_source(
        CodecId(0),
        SourceId("cl-lite-landmark".to_owned()),
        source,
        &mut budget,
    )
    .ok()
    .map(|tree| tree.expr)
}

fn expr_from_expected_display(display: &str) -> Option<Expr> {
    match display {
        "Expr::Bool(true)" => Some(Expr::Bool(true)),
        "Expr::Bool(false)" => Some(Expr::Bool(false)),
        _ => None,
    }
}

fn coverage_level(coverage_percent: f32) -> u8 {
    if !coverage_percent.is_finite() || coverage_percent <= 0.0 {
        return 0;
    }
    (coverage_percent / 20.0).floor().min(5.0) as u8
}

#[cfg(test)]
mod tests {
    use sim_kernel::NumberLiteral;

    use super::*;

    fn report(
        language: Symbol,
        seed: Vec<Expr>,
        unmet_landmarks: Vec<Expr>,
    ) -> GeneratedCoverageReport {
        GeneratedCoverageReport {
            language,
            sampled: 4,
            round_tripped: 4,
            mismatched: 0,
            diagnostics: 0,
            max_depth: 2,
            seed,
            landmark_reproduced: unmet_landmarks.is_empty(),
            unmet_landmarks,
        }
    }

    #[test]
    fn anchored_report_publishes_clamped_coverage() {
        let landmark = Expr::Bool(true);
        let corpus = LandmarkCorpus::new(
            Symbol::new("scheme"),
            Symbol::new("r7rs-small"),
            2,
            vec![landmark.clone()],
        );
        let report = report(Symbol::new("scheme"), vec![landmark], Vec::new());

        let verdict = corpus.reconcile(&report);

        assert_eq!(
            verdict,
            CoverageVerdict::Anchored {
                citation: Symbol::new("r7rs-small"),
                coverage_percent: 100.0,
                curated_fidelity_level: 2,
                claim_level: 2,
            }
        );
    }

    #[test]
    fn missing_landmark_is_unanchored() {
        let corpus = LandmarkCorpus::new(
            Symbol::new("scheme"),
            Symbol::new("r7rs-small"),
            1,
            vec![Expr::Bool(true)],
        );
        let report = report(Symbol::new("scheme"), vec![Expr::Bool(false)], Vec::new());

        let verdict = corpus.reconcile(&report);

        assert!(matches!(
            verdict,
            CoverageVerdict::Unanchored { citation, reason }
                if citation == Symbol::new("r7rs-small")
                    && reason.contains("absent from run")
        ));
    }

    #[test]
    fn unmet_landmark_is_unanchored() {
        let landmark = Expr::Bool(true);
        let corpus = LandmarkCorpus::new(
            Symbol::new("scheme"),
            Symbol::new("r7rs-small"),
            1,
            vec![landmark.clone()],
        );
        let report = report(
            Symbol::new("scheme"),
            vec![landmark.clone()],
            vec![landmark],
        );

        let verdict = corpus.reconcile(&report);

        assert!(matches!(
            verdict,
            CoverageVerdict::Unanchored { reason, .. }
                if reason.contains("not reproduced")
        ));
    }

    #[test]
    fn scheme_corpus_uses_curated_expr_landmarks() {
        let corpus = r7rs_small_landmark_corpus();

        assert_eq!(corpus.language, Symbol::new("scheme"));
        assert_eq!(corpus.citation, Symbol::new("r7rs-small"));
        assert_eq!(corpus.landmarks, vec![Expr::Bool(true)]);
    }

    #[test]
    fn common_lisp_corpus_uses_curated_row_sources() {
        let corpus = common_lisp_lite_landmark_corpus();

        assert_eq!(corpus.language, Symbol::new("common-lisp"));
        assert_eq!(corpus.citation, Symbol::new("cl-hyperspec"));
        assert!(
            corpus
                .landmarks
                .contains(&Expr::Symbol(Symbol::new("profile")))
        );
        assert!(
            corpus
                .landmarks
                .iter()
                .any(|expr| { matches!(expr, Expr::List(items) if items.len() == 2) })
        );
    }

    #[test]
    fn zero_or_invalid_coverage_levels_do_not_raise_claims() {
        assert_eq!(coverage_level(0.0), 0);
        assert_eq!(coverage_level(f32::NAN), 0);
        assert_eq!(coverage_level(19.9), 0);
        assert_eq!(coverage_level(20.0), 1);
        assert_eq!(coverage_level(100.0), 5);
        assert_eq!(coverage_level(250.0), 5);

        let numeric = Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        });
        let corpus = LandmarkCorpus::new(
            Symbol::new("scheme"),
            Symbol::new("r7rs-small"),
            5,
            vec![numeric.clone()],
        );
        let mut report = report(Symbol::new("scheme"), vec![numeric], Vec::new());
        report.round_tripped = 0;

        assert_eq!(
            corpus.reconcile(&report),
            CoverageVerdict::Anchored {
                citation: Symbol::new("r7rs-small"),
                coverage_percent: 0.0,
                curated_fidelity_level: 5,
                claim_level: 0,
            }
        );
    }
}
