#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Manifest-frozen JVM profile boundary.
//!
//! Its checked manifests and fixtures freeze the supported substrate. JVM-owned
//! policy is then layered over shared machine storage, limits, managed handles,
//! kernel values, and the language-neutral raised envelope.

mod class_space;
mod failure;
mod limits;
mod managed;
mod value;

pub use class_space::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, LazyClass,
    class_load_capability,
};
pub use failure::{
    AdmissionFailure, FailureCondition, FailureHome, JavaThrowable, ResourceFailure,
};
pub use limits::{ExecutionLimits, ResourceLimits};
pub use managed::{JVM_ROLE_EDGE_TABLE, JvmEdge, JvmGraphError, JvmHeap, JvmRole, JvmRoleEdges};
pub use value::{JvmReference, JvmValue, JvmValueWidth, PrimitiveCategory, ReturnCategory};

/// The mechanically checked reuse ledger frozen before guest semantics land.
pub const REUSE_LEDGER: &str = include_str!("../reuse-ledger.toml");

/// The admitted classfile baseline and explicit unsupported inventory.
pub const SUPPORTED_RUNTIME: &str = include_str!("../supported-runtime.toml");

/// The closed, initially empty intrinsic manifest.
pub const INTRINSIC_MANIFEST: &str = include_str!("../intrinsics.toml");
