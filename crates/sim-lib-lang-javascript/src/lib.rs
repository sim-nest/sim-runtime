#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Thin, direct JavaScript core profile over lowered `codec/javascript` forms.
//!
//! Syntax remains owned by the codec. This crate owns only bounded ECMAScript
//! policy and composes shared organs, number domains, managed storage, and the
//! tracing collector; it contains no compiler, VM, Realm engine, or host loop.

// conformance: the crate test suite checks the bounded JavaScript core profile.

mod collections;
mod fidelity;
mod jobs;
mod json;
mod managed;
mod matrix_row;
mod modules;
mod objects;
mod profile;
mod regexp;
mod runtime;
mod text;

pub use collections::{
    JavascriptArray, JavascriptCollectionError, JavascriptIterator, JavascriptMap, JavascriptSet,
    JavascriptSymbol, JavascriptSymbolRegistry,
};
pub use fidelity::{
    ECMA262_ORACLE, JavascriptFidelityDimension, JavascriptRegressionCase, TEST262_ORACLE,
    javascript_fidelity_dimensions, javascript_regression_cases,
};
pub use jobs::{
    JavascriptAsyncFunction, JavascriptExceptionRealm, JavascriptGenerator, JavascriptJobClass,
    JavascriptJobs, JavascriptPromise, JavascriptPromiseState,
};
pub use json::{
    JavascriptJsonError, JavascriptJsonValue, JsonReplacer, JsonReviver, JsonToJson,
    parse_javascript_json, stringify_javascript_json,
};
pub use managed::{
    JavascriptHeap, JavascriptHeapExt, JavascriptHeapPolicy, JavascriptManagedKind,
    JavascriptManagedMutationError, JavascriptManagedObject,
};
pub use matrix_row::{javascript_core_matrix_row, javascript_core_source_cases};
pub use modules::{
    dynamic_javascript_policy, dynamic_javascript_policy_with_codec, javascript_module_policy,
    javascript_module_policy_with_codec,
};
pub use objects::{
    JavascriptCallError, JavascriptFunction, JavascriptFunctionKind, JavascriptFunctionPolicy,
    JavascriptObjectError, JavascriptObjectGap, JavascriptObjects, JavascriptPropertyKey,
    JavascriptThis, javascript_callable_shape_constraints, javascript_object_gaps,
};
pub use profile::{
    JavascriptIntrinsic, install_javascript_core_profile, javascript_core_profile,
    javascript_gap_catalog, javascript_intrinsic_manifest, javascript_runtime_kit,
};
pub use regexp::{
    JAVASCRIPT_REGEXP_SUCCESSOR, JavascriptRegExp, JavascriptRegExpError, JavascriptRegExpGap,
    javascript_regexp_gaps,
};
pub use runtime::{Completion, JavascriptEvalPolicy, JavascriptState, JavascriptValue};
pub use text::{JavascriptCodeUnitString, JavascriptTextError};

/// Cookbook recipes embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
