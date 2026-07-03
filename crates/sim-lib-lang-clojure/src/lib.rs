#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Clojure surface profile for the SIM runtime.
//!
//! The kernel defines the codec, eval-policy, and `Expr` contracts; this crate
//! is a loadable language profile presenting a Clojure/EDN surface syntax over
//! the shared `Expr` graph, not a standalone interpreter.

mod codec;
mod conformance;
mod data;
mod matrix_row;
mod profile;
mod reader;
mod recur;
mod symbols;

pub use codec::{ClojureEdnCodec, ClojureEdnCodecLib};
pub use conformance::{run_clojure_core_conformance_case, run_clojure_core_matrix_row};
pub use data::{
    ClojureReducer, clojure_core_namespace, clojure_persistent_data, clojure_profile_sequence,
    clojure_transduce, edn_expr_to_value,
};
pub use matrix_row::{clojure_core_matrix_row, clojure_core_source_cases};
pub use profile::{clojure_core_profile, install_clojure_core_profile};
pub use reader::{decode_clojure_edn_tree, parse_clojure_edn_source};
pub use recur::{clojure_loop_prompt, clojure_loop_prompt_ref, clojure_recur};
pub use symbols::{
    clojure_conformance_test_symbol, clojure_control_fidelity_symbol,
    clojure_core_namespace_symbol, clojure_core_profile_symbol, clojure_edn_reader_symbol,
    clojure_lowering_symbol, clojure_namespace_fidelity_symbol, clojure_recur_prompt_symbol,
    clojure_sequence_fidelity_symbol,
};

#[cfg(test)]
mod tests;
