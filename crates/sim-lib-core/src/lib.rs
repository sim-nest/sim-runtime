#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Shared surface-pack substrate for SIM runtime libraries.
//!
//! The kernel defines the `Lib`/`Registry`/`ExportRecord` contracts; this crate
//! supplies the shared substrate for declaring exported value cards as data and
//! installing them once, idempotently, into a registry.

use sim_kernel::Symbol;

pub mod surface;

/// Recipes embedded at build time from this crate's `recipes/` tree.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use surface::{
    SurfaceField, SurfacePackLib, SurfacePackSpec, SurfaceValueSpec, card_expr, install_once,
    install_once_id, installed_lib_id,
};

/// Returns the manifest name under which this surface pack installs (`lisp:core`).
///
/// # Examples
///
/// ```
/// use sim_kernel::Symbol;
///
/// assert_eq!(sim_lib_core::manifest_name(), Symbol::qualified("lisp", "core"));
/// ```
pub fn manifest_name() -> Symbol {
    Symbol::qualified("lisp", "core")
}
