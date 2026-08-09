#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Thin, direct Python core profile over lowered `codec/python` expressions.
//!
//! This crate intentionally has no instruction format, compiler, or foreign
//! runtime. The codec owns syntax; this profile evaluates its stable token
//! lowering and composes the shared binding, control, mutation, sequence,
//! dispatch, number, arena, and tracing-collector contracts.

mod managed;
mod matrix_row;
mod profile;
mod runtime;

pub use managed::{PythonHeap, PythonHeapPolicy, PythonManagedObject};
pub use matrix_row::{python_core_matrix_row, python_core_source_cases};
pub use profile::{install_python_core_profile, python_core_profile, python_profile_symbol};
pub use runtime::{Annotation, PythonEvalPolicy, PythonFunction, PythonValue};

/// Cookbook recipes for this profile, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
