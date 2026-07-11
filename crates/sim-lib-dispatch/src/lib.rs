#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Generic functions and method dispatch for the SIM runtime.
//!
//! The kernel defines the callable and operation contracts; this crate supplies
//! the concrete dispatch organ (generic functions, multimethods, method
//! specificity ordering).

mod claims;
mod generic;
mod method;
mod runtime;

/// Cookbook recipes for the dispatch organ, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use claims::{
    dispatch_combine_op_key, dispatch_generic_op_key, dispatch_inspect_op_key,
    dispatch_multimethod_op_key, dispatch_op_keys, dispatch_organ_symbol,
    dispatch_specificity_op_key, publish_dispatch_organ_claims,
    publish_dispatch_organ_claims_for_lib,
};
pub use generic::{GenericFunction, Multimethod};
pub use method::{DispatchMethod, MethodBody, MethodRole, MethodSpecificity, compare_specificity};
pub use runtime::generic_function_value;

#[cfg(test)]
mod tests;
