#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Scheme (R7RS-small) surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Scheme surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod card;
mod codec;
mod conformance;
mod forms;
mod lowering;
mod matrix_row;
mod profile;
mod reader;
mod symbols;

pub use card::{scheme_language_card, scheme_language_card_with_generated_coverage};
pub use codec::{SchemeCodec, SchemeCodecLib};
pub use conformance::{run_r7rs_small_conformance_case, run_scheme_matrix_row};
pub use forms::{
    SchemeBaseExport, SchemeFormSpec, SchemeFormStatus, publish_scheme_base_claims,
    publish_scheme_base_claims_for_lib, r7rs_small_base_exports, r7rs_small_form_specs,
};
pub use lowering::{LocatedSchemeLowering, SchemeLowered, lower_scheme_expr, lower_scheme_tree};
pub use matrix_row::{r7rs_small_expr_cases, r7rs_small_matrix_row, r7rs_small_stub_cases};
pub use profile::{
    diagnose_unsupported_forms, install_r7rs_small_profile, r7rs_small_profile,
    run_r7rs_small_restricted,
};
pub use reader::{decode_scheme_tree, parse_scheme_source};
pub use symbols::{
    r7rs_small_profile_symbol, scheme_base_export_kind_symbol, scheme_conformance_test_symbol,
    scheme_lowering_symbol, scheme_reader_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
