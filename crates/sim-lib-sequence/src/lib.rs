#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Sequence behavior for the SIM runtime: lazy, persistent, and transducers.
//!
//! The kernel defines the operation and object contracts; this crate supplies
//! the concrete sequence organ (lazy sequences, persistent vectors/maps/sets,
//! and transducer pipelines).

mod claims;
mod lazy;
mod persistent;
mod profile;
mod runtime;
mod transducer;

pub use claims::{
    publish_sequence_organ_claims, publish_sequence_organ_claims_for_lib, sequence_filter_op_key,
    sequence_for_op_key, sequence_lazy_op_key, sequence_map_op_key, sequence_op_keys,
    sequence_organ_symbol, sequence_persistent_op_key, sequence_reduce_op_key,
    sequence_transduce_op_key,
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
pub use runtime::{SeqOp, SequenceFunction, SequenceLib, install_sequence_lib, sequence_exports};
pub use transducer::{
    TransducerPipeline, TransducerStep, filter_sequence, for_each_sequence, map_sequence,
    reduce_sequence, transduce,
};

#[cfg(test)]
mod tests;
