#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Manifest-frozen JVM profile boundary.
//!
//! This initial crate deliberately contains no guest semantics. Its checked
//! manifests, fixtures, and dependency test freeze the substrate that later
//! phases will compose.

/// The mechanically checked reuse ledger frozen before guest semantics land.
pub const REUSE_LEDGER: &str = include_str!("../reuse-ledger.toml");

/// The admitted classfile baseline and explicit unsupported inventory.
pub const SUPPORTED_RUNTIME: &str = include_str!("../supported-runtime.toml");

/// The closed, initially empty intrinsic manifest.
pub const INTRINSIC_MANIFEST: &str = include_str!("../intrinsics.toml");
