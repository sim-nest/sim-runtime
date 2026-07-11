#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Common Lisp surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Common Lisp surface syntax over
//! the shared `Expr` graph, not a standalone interpreter.

mod codec;
mod conformance;
mod forms;
mod matrix_row;
mod profile;
mod reader;
mod runtime;
mod symbols;

pub use codec::{ClLiteReaderCodec, ClLiteReaderCodecLib};
pub use conformance::{run_cl_lite_conformance_case, run_cl_lite_matrix_row};
pub use forms::{ClLiteFormRole, ClLiteFormSpec, cl_lite_form_specs};
pub use matrix_row::{cl_lite_matrix_row, cl_lite_source_cases};
pub use profile::{cl_lite_profile, install_cl_lite_profile};
pub use reader::{decode_cl_lite_tree, parse_cl_lite_source};
pub use runtime::{
    ClFunctionBody, ClGenericFunction, ClLiteControlScope, ClLiteRuntime, call_cl_value,
    cl_lite_package,
};
pub use symbols::{
    cl_binding_fidelity_symbol, cl_clos_mop_fidelity_symbol, cl_conformance_test_symbol,
    cl_control_fidelity_symbol, cl_dispatch_fidelity_symbol, cl_lite_package_symbol,
    cl_lite_profile_symbol, cl_lowering_symbol, cl_mutation_fidelity_symbol,
    cl_namespace_fidelity_symbol, cl_reader_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
