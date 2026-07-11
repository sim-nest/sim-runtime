#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Typed, lazy surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a typed, lazily-evaluated surface
//! syntax over the shared `Expr` graph, not a standalone interpreter.

mod conformance;
mod matrix_row;
mod profile;
mod runtime;
mod symbols;

pub use conformance::{run_typed_lazy_conformance_case, run_typed_lazy_matrix_row};
pub use matrix_row::{typed_lazy_matrix_row, typed_lazy_source_cases};
pub use profile::{install_typed_lazy_profile, typed_lazy_profile};
pub use runtime::{LazyRef, TypeclassDictionary, typed_lazy_option_type};
pub use symbols::{
    typed_lazy_conformance_test_symbol, typed_lazy_control_fidelity_symbol,
    typed_lazy_lowering_symbol, typed_lazy_pattern_fidelity_symbol, typed_lazy_profile_symbol,
    typed_lazy_reader_symbol, typed_lazy_typeclass_fidelity_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
