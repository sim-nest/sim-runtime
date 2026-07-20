#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Sequence behavior for the SIM runtime: lazy, persistent, runtime-indexed,
//! and transducer-backed collections.
//!
//! The kernel defines the operation and object contracts; this crate supplies
//! the concrete sequence organ (lazy sequences, persistent vectors/maps/sets,
//! runtime-indexed projection, and transducer pipelines).

mod claims;
mod lazy;
mod persistent;
mod profile;
mod runtime;
mod runtime_iter;
mod transducer;

pub use claims::{
    publish_sequence_organ_claims, publish_sequence_organ_claims_for_lib,
    sequence_declared_op_keys, sequence_filter_op_key, sequence_for_op_key, sequence_lazy_op_key,
    sequence_live_ops, sequence_map_op_key, sequence_op_keys, sequence_organ_symbol,
    sequence_persistent_op_key, sequence_reduce_op_key, sequence_transduce_op_key,
};
pub use lazy::{
    LazySequence, SequenceProducer, force_sequence_bounded, lazy_sequence_value,
    sequence_from_list_value,
};
pub use persistent::{
    PersistentSet, PersistentVector, persistent_list, persistent_list_push, persistent_map,
    persistent_map_assoc, persistent_set, persistent_set_insert, persistent_vector,
    persistent_vector_push,
};
pub use profile::{ProfileSequence, sequence_for_profile};
pub use runtime::{
    SeqOp, SequenceFunction, SequenceLib, install_sequence_lib, manifest_name, sequence_exports,
};
pub use runtime_iter::{
    RuntimeIndexLookup, RuntimeIndexSource, runtime_index_lookup_sequence, runtime_index_sequence,
    runtime_index_values,
};
pub use transducer::{
    TransducerPipeline, TransducerStep, filter_sequence, for_each_sequence, map_sequence,
    reduce_sequence, transduce,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
