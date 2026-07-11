#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Lua surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Lua surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod conformance;
mod matrix_row;
mod profile;
mod runtime;
mod symbols;

pub use conformance::{run_lua_core_conformance_case, run_lua_core_matrix_row};
pub use matrix_row::{lua_core_matrix_row, lua_core_source_cases};
pub use profile::{install_lua_core_profile, lua_core_profile};
pub use runtime::{lua_coroutine, lua_table, lua_table_value};
pub use symbols::{
    lua_conformance_test_symbol, lua_control_fidelity_symbol, lua_full_runtime_fidelity_symbol,
    lua_lowering_symbol, lua_mutation_fidelity_symbol, lua_profile_symbol, lua_reader_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
