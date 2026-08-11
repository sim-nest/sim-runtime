#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Thin, direct Python core profile over lowered `codec/python` expressions.
//!
//! This crate intentionally has no instruction format, compiler, or foreign
//! runtime. The codec owns syntax; this profile evaluates its stable token
//! lowering and composes the shared binding, control, mutation, sequence,
//! dispatch, number, arena, and tracing-collector contracts.

// conformance: the crate test suite checks the authorized Python library core.

mod fidelity;
mod library_core;
mod managed;
mod matrix_row;
mod objects;
mod profile;
mod resumable;
mod runtime;

#[cfg(test)]
mod tests;

pub use fidelity::{
    PYTHON_EVIDENCE_CASES, PYTHON_EXTERNAL_ORACLE, PYTHON_FIDELITY, PythonEvidenceCase,
    PythonFidelity,
};
pub use library_core::{
    DynamicAdmission, DynamicPython, MatchCase, MatchOutcome, PythonLibraryManifest,
    PythonModuleAdmission, PythonModulePolicy, PythonSurface, PythonSurfaceState, match_expr,
    python_library_manifest,
};
pub use managed::{PythonHeap, PythonHeapPolicy, PythonManagedKind, PythonManagedObject};
pub use matrix_row::{python_core_matrix_row, python_core_source_cases};
pub use objects::{
    AttributeError, ClassError, DescriptorHook, PythonClass, PythonObjectSpace, PythonObjectValue,
};
pub use profile::{install_python_core_profile, python_core_profile, python_profile_symbol};
pub use resumable::{
    ContextManager, PythonException, PythonExceptionGroup, PythonGenerator, PythonGeneratorError,
    PythonGeneratorStep, PythonIterator, run_with_context,
};
pub use runtime::{Annotation, PythonEvalPolicy, PythonFunction, PythonValue};

/// Deliberately unsupported Python object and control edges.
pub const PYTHON_OBJECT_CONTROL_GAPS: &[&str] = &[
    "custom metaclass construction and __prepare__",
    "dynamic descriptor protocol mutation after class creation",
    "weak-reference callbacks and proxy objects",
    "language-visible __del__ finalizers and resurrection",
    "async event-loop scheduling and async-generator hooks",
];

/// A scheduler-free Python coroutine frame using the same checked send/throw
/// transition contract as a generator.
pub type PythonCoroutine<T, D> = PythonGenerator<T, D>;

/// Cookbook recipes for this profile, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
