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
