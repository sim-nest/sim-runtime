#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Pattern behavior for the SIM runtime over the kernel `Shape` protocol.
//!
//! The kernel defines the `Shape` match/binding protocol; this crate supplies
//! the concrete pattern organ (algebraic data types, destructuring, match arms,
//! and exhaustiveness checking) as pattern surfaces over that protocol.

mod adt;
mod claims;
mod match_form;
mod matching;
mod runtime;
mod shapes;

pub use adt::{
    AlgebraicDataType, PatternField, TaggedValue, VariantConstructor, VariantDeclaration,
    tagged_value,
};
pub use claims::{
    pattern_adt_op_key, pattern_destructure_op_key, pattern_exhaustive_op_key,
    pattern_match_op_key, pattern_op_keys, pattern_organ_symbol, pattern_tag_op_key,
    publish_pattern_organ_claims, publish_pattern_organ_claims_for_lib,
};
pub use match_form::MatchForm;
pub use matching::{
    MatchArm, PatternMatch, destructure_expr, destructure_value, exhaustiveness_diagnostics,
    match_value,
};
pub use runtime::{PatternLib, install_pattern_lib, manifest_name, pattern_exports};
pub use shapes::{AdtShape, VariantShape};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
