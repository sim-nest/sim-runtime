#![cfg_attr(not(feature = "native-export"), forbid(unsafe_code))]
#![cfg_attr(feature = "native-export", deny(unsafe_code))]
#![deny(missing_docs)]
//! Standard distribution core for the SIM runtime.
//!
//! The kernel defines the capability, claim, codec, and `ExportRecord`
//! contracts; this crate supplies the standard-distribution behavior:
//! capabilities, claims, diff, fidelity, the conformance harness, install,
//! language-profile support, the lisp codec surface, polyglot/profile support,
//! and read/construct.

pub mod cap;
pub mod claims;
pub mod diff;
pub mod fidelity;
pub mod harness;
pub mod install;
pub mod lang_profile;
pub mod lisp;
pub mod matrix;
mod matrix_claims;
#[cfg(feature = "native-export")]
mod native;
pub mod polyglot;
pub mod profile;
pub mod read_construct;
pub mod registry;

pub use cap::{standard_diff_capability, standard_install_capability, standard_test_capability};
pub use claims::{
    publish_badge_claims, publish_badge_claims_for_lib, publish_profile_claims,
    publish_profile_claims_for_lib, standard_capability_predicate, standard_eval_policy_predicate,
    standard_lowering_predicate, standard_numeric_predicate, standard_reader_predicate,
    standard_unsupported_predicate,
};
pub use diff::{
    ProfileDiff, ProfileDiffStatus, ProfileDifference, profile_diff_symbol, standard_diff_op_key,
    standard_diff_stub,
};
pub use fidelity::{FidelityBadge, fidelity_badge_class_symbol};
pub use harness::{
    ConformanceHarness, ConformanceOutcome, ConformanceStatus, ConformanceTestCase,
    ConformanceTestReport, OrganTestReport, StandardTestReport,
    standard_reported_fidelity_level_predicate, standard_reported_fidelity_predicate,
    standard_test_case_predicate, standard_test_op_key, standard_test_organ_predicate,
    standard_test_profile_predicate, standard_test_result_predicate, standard_test_run_kind,
    standard_test_status_predicate, standard_test_stub,
};
pub use install::{StandardInstallReport, install_profile_stub, standard_install_op_key};
pub use lang_profile::{
    FidelityBadgeSpec, ProfileBackingLib, ProfileOrganPublisher, fidelity_badge,
    install_language_profile, language_profile_lib_symbol,
};
pub use lisp::{lisp_stub_symbols, standard_fidelity_symbol, standard_profile_symbol};
pub use matrix::{
    ConformanceMatrix, ExprRoundTripCase, ExprRoundTripObservation, LanguageRow,
    LanguageRowBuilder, MatrixCellKind, MatrixCellResult, MatrixRunReport, MatrixRunner,
    SourceConformanceCase, SourceConformanceCaseKind, SourceExpectation, SourceObservation,
    compare_expr_observation, compare_source_observation,
};
pub use polyglot::{
    ProfileFunction, ProfileFunctionBinding, SharedOrganRuntime, profile_function_value,
};
pub use profile::{
    LanguageProfile, OrganUse, language_profile_class_symbol, sim_expression_profile,
    sim_expression_profile_symbol, standard_binding_organ_symbol, standard_control_organ_symbol,
    standard_pattern_organ_symbol, standard_sequence_organ_symbol,
};
pub use read_construct::{
    FidelityBadgeValue, LanguageProfileValue, install_standard_core_classes,
    standard_core_classes_lib_symbol,
};
pub use registry::ProfileRegistry;

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod harness_tests;
#[cfg(test)]
mod matrix_tests;
#[cfg(test)]
mod tests;
