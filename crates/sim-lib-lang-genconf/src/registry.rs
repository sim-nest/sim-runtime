//! Registry for generated language-conformance rows.

use sim_kernel::{Cx, Symbol};

use crate::{ExprSpace, GeneratedCoverageReport, run_generated_row};

/// One generated conformance row: language id, codec id, and expression space.
pub struct GenerativeRow {
    /// Language row measured by this generated conformance entry.
    pub language: Symbol,
    /// Codec for encoding and decoding generated expressions for this row.
    pub codec: Symbol,
    /// Expression space sampled for this row.
    pub space: ExprSpace,
}

/// Returns every language codec measured by generated conformance.
///
/// Adding a language is adding one row here with the language row id, reader
/// codec id, and shared expression space.
pub fn generative_registry() -> Vec<GenerativeRow> {
    vec![
        row("scheme", "scheme-r7rs-small"),
        row("common-lisp", "common-lisp-lite"),
        row("clojure", "clojure-edn"),
        row("islisp", "islisp"),
        row("julia", "algol"),
        row("lua", "algol"),
        row("ruby", "algol"),
        row("typed-lazy", "algol"),
        core_round_trip_row("prolog", "lisp"),
    ]
}

/// Runs generated conformance for every registered language row.
pub fn run_all_generated(cx: &mut Cx, budget: usize) -> Vec<GeneratedCoverageReport> {
    generative_registry()
        .into_iter()
        .map(|row| run_generated_row(cx, &row.language, &row.codec, &row.space, budget))
        .collect()
}

fn row(language: &str, codec: &str) -> GenerativeRow {
    GenerativeRow {
        language: Symbol::new(language),
        codec: Symbol::qualified("codec", codec),
        space: ExprSpace::r7rs_core_space(3),
    }
}

fn core_round_trip_row(language: &str, codec: &str) -> GenerativeRow {
    GenerativeRow {
        language: Symbol::new(language),
        codec: Symbol::qualified("codec", codec),
        space: ExprSpace::core_round_trip_space(3),
    }
}

#[cfg(test)]
mod closure_tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Symbol};
    use sim_lib_lang_matrix::language_matrix;

    use super::*;

    fn closure_cx() -> Cx {
        let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        sim_test_support::register_core_classes(&mut cx);
        sim_test_support::register_f64_number_domain(&mut cx);
        cx
    }

    #[test]
    fn closure_registry_tracks_the_curated_language_matrix() {
        let registry = generative_registry();
        let matrix = language_matrix();
        let mut names = BTreeSet::new();

        assert_eq!(registry.len(), matrix.language_count());
        assert_eq!(matrix.language_count(), 9);
        for row in &registry {
            assert!(
                names.insert(row.language.clone()),
                "duplicate generated row: {}",
                row.language,
            );
            assert!(
                matrix.row(&row.language).is_some(),
                "{} row must stay in the curated matrix",
                row.language,
            );
        }
    }

    #[test]
    fn closure_registry_uses_live_language_codec_ids() {
        let registry = generative_registry();

        assert!(registry.iter().any(|row| {
            row.language == Symbol::new("scheme")
                && row.codec == Symbol::qualified("codec", "scheme-r7rs-small")
        }));
        assert!(registry.iter().any(|row| {
            row.language == Symbol::new("common-lisp")
                && row.codec == Symbol::qualified("codec", "common-lisp-lite")
        }));
        assert!(registry.iter().any(|row| {
            row.language == Symbol::new("prolog") && row.codec == Symbol::qualified("codec", "lisp")
        }));
    }

    #[test]
    fn closure_reports_are_anchored_or_honest() {
        let mut cx = closure_cx();
        let reports = run_all_generated(&mut cx, 8);

        assert_eq!(reports.len(), generative_registry().len());
        for report in &reports {
            if !report.landmark_reproduced() {
                assert!(
                    report.coverage_percent().is_none(),
                    "{} must not publish unanchored coverage",
                    report.language,
                );
            }
        }
        assert_eq!(language_matrix().language_count(), 9);
    }
}
