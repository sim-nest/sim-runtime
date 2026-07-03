#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Binding behavior for the SIM runtime: lexical, dynamic, and scoped binding.
//!
//! The kernel defines the binding-related contracts; this crate supplies the
//! concrete binding organ (lexical/letrec scopes, dynamic parameters, modes).

mod claims;
mod dynamic;
mod lexical;
mod modes;

/// Cookbook recipes for the binding organ, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use claims::{
    binding_dynamic_let_op_key, binding_let_op_key, binding_let_star_op_key, binding_letrec_op_key,
    binding_op_keys, binding_organ_symbol, binding_parameterize_op_key,
    binding_profile_modes_op_key, publish_binding_organ_claims,
    publish_binding_organ_claims_for_lib,
};
pub use dynamic::{DynamicEnv, Parameter};
pub use lexical::{
    BindingInitializer, LexicalEnv, LexicalFunction, eval_let, eval_let_star, eval_letrec,
    lexical_function_value,
};
pub use modes::{BindingProfileModes, BindingScopeMode, HygieneMode};

#[cfg(test)]
mod tests;
