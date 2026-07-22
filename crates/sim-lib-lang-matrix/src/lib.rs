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

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod closure_tests;
#[cfg(test)]
mod tests;
