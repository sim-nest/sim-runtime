#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Julia surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Julia surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod conformance;
mod generic;
mod matrix_row;
mod profile;
mod symbols;

pub use conformance::{run_julia_core_conformance_case, run_julia_core_matrix_row};
pub use generic::JuliaFunction;
pub use matrix_row::{julia_core_matrix_row, julia_core_source_cases};
pub use profile::{install_julia_core_profile, julia_core_profile};
pub use symbols::{
    julia_conformance_test_symbol, julia_dispatch_fidelity_symbol,
    julia_full_runtime_fidelity_symbol, julia_lowering_symbol, julia_profile_symbol,
    julia_reader_symbol,
};

#[cfg(test)]
mod tests;
