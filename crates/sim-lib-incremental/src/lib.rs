#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Loadable incremental query organ for SIM runtime expressions.
//!
//! This crate wraps [`sim_incremental_core`] as a runtime library that owns the
//! SIM boundary: query source expressions are registered as `Expr`, verified
//! through the generic core, and projected back as ordinary runtime values. The
//! core crate remains independent of SIM value representation and library
//! loading.

mod cap;
mod claims;
mod model;
mod runtime;
mod shapes;

pub use cap::{
    incremental_read_capability, incremental_verify_capability, incremental_write_capability,
};
pub use claims::{
    incremental_engine_op_key, incremental_explain_op_key, incremental_invalidate_op_key,
    incremental_metrics_op_key, incremental_op_keys, incremental_organ_symbol,
    incremental_register_op_key, incremental_snapshot_op_key, incremental_verify_op_key,
    publish_incremental_organ_claims, publish_incremental_organ_claims_for_lib,
};
pub use model::{IncrementalSession, incremental_engine_value};
pub use runtime::{IncrementalLib, incremental_exports, install_incremental_lib};
pub use shapes::{
    incremental_engine_shape_symbol, incremental_key_shape_symbol,
    incremental_query_expr_shape_symbol, incremental_report_shape_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
