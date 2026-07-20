#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Lua surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Lua surface syntax over the
//! shared `Expr` graph, not a standalone interpreter.

mod call;
mod closure;
mod conformance;
mod env;
mod eval;
mod forms;
mod load;
mod loops;
mod matrix_row;
mod metatable;
mod number;
mod operator;
mod pattern_replace;
mod profile;
mod runtime;
mod stdlib_base;
mod stdlib_coroutine;
mod stdlib_debug;
mod stdlib_io;
mod stdlib_math;
mod stdlib_os;
mod stdlib_package;
mod stdlib_string;
mod stdlib_string_format;
mod stdlib_string_pattern;
mod stdlib_table;
mod stdlib_utf8;
mod symbols;
mod table;
mod value;

pub use conformance::{run_lua_core_conformance_case, run_lua_core_matrix_row};
pub use env::LuaEnv;
pub use eval::LuaEvalPolicy;
pub use matrix_row::{lua_core_matrix_row, lua_core_source_cases};
pub use metatable::{lua_get, lua_index_slot, lua_metamethod};
pub use number::{LuaNumber, lua_float_value, lua_integer_value, lua_number_from_value};
pub use operator::{LuaOp, lua_binary, lua_len};
pub use profile::{install_lua_core_profile, lua_core_profile};
pub use runtime::lua_coroutine;
pub use stdlib_coroutine::{LuaThread, lua_coroutine_frame_value};
pub use symbols::{
    lua_conformance_test_symbol, lua_control_fidelity_symbol, lua_eval_policy_symbol,
    lua_full_runtime_fidelity_symbol, lua_lowering_symbol, lua_mutation_fidelity_symbol,
    lua_profile_symbol, lua_reader_symbol,
};
pub use table::{
    LuaTable, LuaTablePolicy, lua_get_metatable, lua_rawdel, lua_rawget, lua_rawset,
    lua_set_metatable, lua_table, lua_table_from_values, lua_table_value,
};
pub use value::LuaResult;

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod lua3_12_tests;

#[cfg(test)]
mod lua3_13_tests;

#[cfg(test)]
mod lua3_14_tests;

#[cfg(test)]
mod tests;
