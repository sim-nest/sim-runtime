#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Mutation behavior for the SIM runtime: cells, boxes, vectors, symbol-keyed
//! tables, and runtime-keyed tables.
//!
//! The kernel defines the capability and operation contracts; this crate
//! supplies the concrete mutation organ (mutable cells, boxes, vectors,
//! symbol-keyed tables, and runtime-keyed tables) guarded by a standard mutate
//! capability. Every in-place write goes through
//! [`standard_mutate_capability`] so mutation stays auditable, and the organ
//! publishes its operation keys as claims via
//! [`publish_mutation_organ_claims`].
//!
//! See the crate [README] for where this organ sits in the constellation.
//!
//! [README]: https://github.com/sim-nest/sim-runtime

mod cap;
mod cell;
mod claims;
mod runtime_key;
mod runtime_table;
mod table;
mod vector;

pub use cap::standard_mutate_capability;
pub use cell::{Cell, MutableBox, cell_value, mutable_box_value};
pub use claims::{
    mutation_box_op_key, mutation_cell_op_key, mutation_op_keys, mutation_organ_symbol,
    mutation_set_op_key, mutation_table_op_key, mutation_vector_op_key,
    publish_mutation_organ_claims, publish_mutation_organ_claims_for_lib,
};
pub use runtime_key::{PrimitiveRuntimeKeyPolicy, RuntimeKey, RuntimeKeyPolicy};
pub use runtime_table::{MutableRuntimeTable, mutable_runtime_table, mutable_runtime_table_value};
pub use table::{MutableTable, mutable_table, mutable_table_value};
pub use vector::{MutableVector, mutable_vector, mutable_vector_from_value, mutable_vector_value};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
