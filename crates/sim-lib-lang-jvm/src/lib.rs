#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Manifest-frozen JVM profile boundary.
//!
//! Its checked manifests and fixtures freeze the supported substrate. JVM-owned
//! policy is then layered over shared machine storage, limits, managed handles,
//! kernel values, and the language-neutral raised envelope.

mod array;
mod class_metadata;
mod class_space;
mod code;
mod control;
mod entry;
mod execution;
mod failure;
mod field;
mod intrinsic;
mod limits;
mod managed;
mod numeric;
mod resolution;
mod text;
mod value;

pub use array::{
    ArrayAllocationError, ArrayComponent, ArrayOperationError, ArrayPrimitive, JavaArray,
    JavaArrayTree, MAX_ARRAY_DIMENSIONS,
};
pub use class_metadata::{
    JavaClassMetadata, JavaHierarchyCheck, JavaMember, JavaMemberKind, JavaResolutionEvidence,
};
pub use class_space::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision, LazyClass,
    class_load_capability,
};
pub use code::{
    JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError,
    PreparedExceptionHandler, PreparedJvmInstruction, PreparedJvmPolicy, RootEffect, prepare_code,
};
pub use control::{
    JvmControlError, JvmControlErrorKind, JvmControlOutcome, execute_control_instruction,
};
pub use entry::{
    ClassfilePermit, EntryRefusal, EntryTarget, ExecutionPermit, NoVerifier, PreparedEntry,
    ResolvedEntry, StaticAdmission, VerificationFidelity, VerifierProvider, drive,
};
pub use execution::{
    ExecutionError, JvmConstantResolver, JvmWorkReceipt, execute_storage_instruction,
};
pub use failure::{
    AdmissionFailure, FailureCondition, FailureHome, JavaThrowable, ResourceFailure,
};
pub use field::{
    FieldAccess, FieldError, FieldId, FieldLayout, FieldStorage, InitializationState, JavaObject,
    WriteContext,
};
pub use intrinsic::{
    BoxValue, INTRINSIC_TABLE, IntrinsicError, IntrinsicMember, IntrinsicSupport, PrimitiveBox,
    PrimitiveBoxes, admit_intrinsic,
};
pub use limits::{ExecutionLimits, ResourceLimits};
pub use managed::{JVM_ROLE_EDGE_TABLE, JvmEdge, JvmGraphError, JvmHeap, JvmRole, JvmRoleEdges};
pub use numeric::{NumericExecutionError, execute_numeric_instruction};
pub use resolution::{
    AccessDecision, ConstantResolution, ConstantResolutionError, ConstantResolutionKind,
    ResolutionCache, RuntimeNest, RuntimePackage,
};
pub use text::{ADMITTED_CORE_MEMBERS, JavaClassMirror, JavaCoreMember, JavaString};
pub use value::{JvmReference, JvmValue, JvmValueWidth, PrimitiveCategory, ReturnCategory};

/// The mechanically checked reuse ledger frozen before guest semantics land.
pub const REUSE_LEDGER: &str = include_str!("../reuse-ledger.toml");

/// The admitted classfile baseline and explicit unsupported inventory.
pub const SUPPORTED_RUNTIME: &str = include_str!("../supported-runtime.toml");

/// The closed intrinsic manifest.
pub const INTRINSIC_MANIFEST: &str = include_str!("../intrinsics.toml");

/// Auditable ownership decision for every JVM numeric instruction family.
pub const NUMERIC_OWNERSHIP: &str = include_str!("../numeric-ownership.toml");

/// Auditable storage-ownership decision for Java arrays.
pub const ARRAY_OWNERSHIP: &str = include_str!("../array-ownership.toml");
