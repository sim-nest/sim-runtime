#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Prolog surface profile for the SIM runtime.
//!
//! The kernel defines the expression and library contracts; this crate installs
//! a Prolog-flavored logic policy and registers the `prolog/*` callable surface
//! over the shared logic query engine.

mod card;
mod conformance;
mod conformance_all_solutions;
mod exports;
#[cfg(feature = "generated-coverage")]
mod generated_coverage;
mod matrix_row;
mod profile;
mod surface;
mod symbols;

pub use card::prolog_language_card;
pub use conformance::{run_prolog_conformance_case, run_prolog_matrix_row};
pub use exports::prolog_exports;
#[cfg(feature = "generated-coverage")]
pub use generated_coverage::{PrologGeneratedCoverage, run_prolog_generated_coverage};
pub use matrix_row::{prolog_conformance_cases, prolog_matrix_row};
pub use profile::{install_prolog_profile, prolog_profile};
pub use surface::{PrologLib, install_prolog_lib};
pub use symbols::{
    prolog_conformance_case_symbol, prolog_conformance_test_symbol, prolog_logic_organ_symbol,
    prolog_lowering_symbol, prolog_profile_symbol, prolog_reader_symbol,
    prolog_surface_fidelity_symbol,
};

#[cfg(test)]
mod tests;
