#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! ISLISP surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting an ISLISP surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod conformance;
mod forms;
mod generic;
mod matrix_row;
mod profile;
mod symbols;

pub use conformance::{run_islisp_conformance_case, run_islisp_matrix_row};
pub use forms::{IslispFormRole, IslispFormSpec, islisp_form_specs};
pub use generic::{IslispGeneric, IslispObject, islisp_object_value};
pub use matrix_row::{islisp_matrix_row, islisp_source_cases};
pub use profile::{install_islisp_profile, islisp_profile};
pub use symbols::{
    islisp_conformance_test_symbol, islisp_dispatch_fidelity_symbol, islisp_lowering_symbol,
    islisp_profile_symbol, islisp_reader_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
