#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! TypeScript notation over the unchanged JavaScript evaluator.
//!
//! The codec owns parsing and direct erasure. This crate retains provenance,
//! projects only a bounded faithful annotation vocabulary to observational
//! Shape metadata, and delegates execution to `sim-lib-lang-javascript`.

// conformance: the crate test suite checks bounded TypeScript notation and erasure.

mod fidelity;
mod metadata;
mod profile;
mod runtime;

pub use fidelity::{
    TYPESCRIPT_EXTERNAL_ORACLE, TypeScriptFidelityDimension, typescript_fidelity_dimensions,
};
pub use metadata::{
    AnnotationMetadata, AnnotationProvenance, ProjectedShape, attach_browse_signature,
    project_annotation,
};
pub use profile::{
    install_typescript_notation_profile, typescript_gap_manifest, typescript_notation_profile,
    typescript_profile_symbol,
};
pub use runtime::{TypeScriptNotation, TypeScriptProgram};

/// Cookbook recipes embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
