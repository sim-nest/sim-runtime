#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Namespace behavior for the SIM runtime: modules, packages, and imports.
//!
//! The kernel defines the registry and operation contracts; this crate supplies
//! the concrete namespace organ (namespaces, import options, export/rename/
//! shadow handling).

mod claims;
mod namespace;

pub use claims::{
    namespace_export_op_key, namespace_import_op_key, namespace_module_op_key, namespace_op_keys,
    namespace_organ_symbol, namespace_package_op_key, namespace_rename_op_key,
    namespace_shadow_op_key, publish_namespace_organ_claims,
    publish_namespace_organ_claims_for_lib,
};
pub use namespace::{
    ImportOptions, Namespace, NamespaceBindingSource, NamespaceEntry, NamespaceKind,
    namespace_shadow_conflict_symbol,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
