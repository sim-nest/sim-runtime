#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Assembly point for the SIM language conformance matrix.

use sim_lib_lang_cl::cl_lite_matrix_row;
use sim_lib_lang_clojure::clojure_core_matrix_row;
use sim_lib_lang_islisp::islisp_matrix_row;
use sim_lib_lang_julia::julia_core_matrix_row;
use sim_lib_lang_lua::lua_core_matrix_row;
use sim_lib_lang_prolog::prolog_matrix_row;
use sim_lib_lang_ruby::ruby_dsl_matrix_row;
use sim_lib_lang_scheme::r7rs_small_matrix_row;
use sim_lib_lang_typed_lazy::typed_lazy_matrix_row;
use sim_lib_standard_core::ConformanceMatrix;

/// Builds the complete runtime language conformance matrix.
pub fn language_matrix() -> ConformanceMatrix {
    let mut matrix = ConformanceMatrix::new();
    matrix.register(r7rs_small_matrix_row());
    matrix.register(cl_lite_matrix_row());
    matrix.register(clojure_core_matrix_row());
    matrix.register(islisp_matrix_row());
    matrix.register(julia_core_matrix_row());
    matrix.register(lua_core_matrix_row());
    matrix.register(ruby_dsl_matrix_row());
    matrix.register(typed_lazy_matrix_row());
    matrix.register(prolog_matrix_row());
    matrix
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sim_kernel::Symbol;

    use super::*;

    #[test]
    fn language_matrix_has_exactly_nine_rows() {
        let matrix = language_matrix();

        assert_eq!(matrix.language_count(), 9);
    }

    #[test]
    fn language_matrix_has_no_duplicate_row_names() {
        let matrix = language_matrix();
        let names: Vec<_> = matrix
            .iter_rows()
            .map(|row| row.language.to_string())
            .collect();
        let unique: BTreeSet<_> = names.iter().cloned().collect();

        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn language_matrix_rows_are_populated() {
        let matrix = language_matrix();

        for language in [
            "scheme",
            "common-lisp",
            "clojure",
            "islisp",
            "julia",
            "lua",
            "ruby",
            "typed-lazy",
            "prolog",
        ] {
            let row = matrix
                .row(&Symbol::new(language))
                .unwrap_or_else(|| panic!("{language} row must be registered"));
            assert!(!row.is_empty(), "{language} row must have cases");
        }
    }
}

#[cfg(test)]
mod closure_tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Symbol};
    use sim_lib_lang_genconf::{generative_registry, run_all_generated};

    use crate::language_matrix;

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
