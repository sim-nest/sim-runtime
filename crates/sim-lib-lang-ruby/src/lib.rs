#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Ruby DSL surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Ruby surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod conformance;
mod matrix_row;
mod profile;
mod runtime;
mod symbols;

pub use conformance::{run_ruby_dsl_conformance_case, run_ruby_dsl_matrix_row};
pub use matrix_row::{ruby_dsl_matrix_row, ruby_dsl_source_cases};
pub use profile::{install_ruby_dsl_profile, ruby_dsl_profile};
pub use runtime::{RubyBlockScope, ruby_break, ruby_next};
pub use symbols::{
    ruby_blocks_fidelity_symbol, ruby_conformance_test_symbol, ruby_control_fidelity_symbol,
    ruby_dispatch_fidelity_symbol, ruby_lowering_symbol, ruby_profile_symbol, ruby_reader_symbol,
};

#[cfg(test)]
mod tests;
