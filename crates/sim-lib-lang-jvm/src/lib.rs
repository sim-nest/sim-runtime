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
mod constructor;
mod control;
mod dynamic;
mod entry;
mod exception;
mod execution;
mod failure;
mod field;
mod initialization;
mod inspection;
mod intrinsic;
mod invocation;
mod limits;
mod linker;
mod managed;
mod monitor;
mod numeric;
mod resolution;
mod specimen;
mod surface;
mod text;
mod value;
mod verifier;

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
    PreparedCatchEntry, PreparedJvmInstruction, PreparedJvmPolicy, RootEffect, prepare_code,
};
pub use constructor::{
    ConstructorState, ConstructorStateError, UninitializedUse, VerifiedConstructorState,
};
pub use control::{
    JvmControlError, JvmControlErrorKind, JvmControlOutcome, execute_control_instruction,
};
pub use dynamic::{
    ConcatConstant, DynamicBootstrap, DynamicLinkCache, DynamicLinkError, LinkedStringConcat,
    STRING_CONCAT_BOOTSTRAP_DESCRIPTOR, STRING_CONCAT_BOOTSTRAP_NAME,
    STRING_CONCAT_BOOTSTRAP_OWNER,
};
pub use entry::{
    ClassVerifierProvider, ClassfilePermit, EntryRefusal, EntryTarget, ExecutionPermit, NoVerifier,
    PreparedEntry, ResolvedEntry, StaticAdmission, VerificationFidelity, VerificationProofFailure,
    VerifierProvider, drive,
};
pub use exception::{
    JavaHandlerEntry, JavaThrowError, JavaThrowSite, JavaThrowableHeap, JavaThrowableMutationError,
    JavaThrowableRelation, JavaThrowableState, execute_athrow, unwind_java_frame,
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
pub use initialization::{
    ClassInitialization, ClassInitializationState, InitializationAction, InitializationError,
    InitializationLane, InitializationPlan, InitializationResume, InitializationSnapshot,
};
pub use inspection::{
    ClassVerificationView, MethodVerificationView, VerificationExplanation, VerificationFrameView,
    VerifierCoverage,
};
pub use intrinsic::{
    BoxValue, INTRINSIC_TABLE, IntrinsicError, IntrinsicMember, IntrinsicSupport, PrimitiveBox,
    PrimitiveBoxes, admit_intrinsic,
};
pub use invocation::{
    InvocationError, InvocationKind, SelectedMethod, call_transfer, return_transfer,
    select_invocation,
};
pub use limits::{ExecutionLimits, ResourceLimits};
pub use linker::{
    AdaptationPoint, BootstrapArgument, BootstrapMethod, DirectHandleError, DirectInvocationKind,
    DirectReceiver, FunctionalInterface, FunctionalInterfaceError, GeneratedLambdaClass,
    GeneratedLambdaClassError, GeneratedLambdaClassSpace, GeneratedLambdaMember,
    GeneratedLambdaMemberRole, JvmAdaptation, JvmAdaptationError, JvmFunctionPlan,
    JvmFunctionPolicyBody, LambdaBootstrapError, LambdaBootstrapPlan, LambdaBootstrapProtocol,
    LambdaProtocolTail, LinkageCache, LinkageFailure, LinkageState, LocatedJvmAdaptation,
    MethodIdentity, ResolvedBootstrapArgument, ResolvedDirectHandle, SiteKey,
    compile_jvm_function_plan, decode_lambda_bootstrap, discover_functional_interface,
    executor_admitted_lambda_protocols, resolve_direct_handle, validate_functional_interface,
};
pub use managed::{JVM_ROLE_EDGE_TABLE, JvmEdge, JvmGraphError, JvmHeap, JvmRole, JvmRoleEdges};
pub use monitor::{MonitorError, MonitorLane, MonitorTable};
pub use numeric::{NumericExecutionError, execute_numeric_instruction};
pub use resolution::{
    AccessDecision, ConstantResolution, ConstantResolutionError, ConstantResolutionKind,
    ResolutionCache, RuntimeNest, RuntimePackage,
};
pub use specimen::{JvmProductSpecimen, run_product_specimen};
pub use surface::{
    JVM_DECLARED_ABSENCES, JvmBrowse, JvmLanguageLib, JvmSurface, install_jvm_language_lib,
    jvm_browse_capability, jvm_invoke_capability, jvm_language_profile,
};
pub use text::{ADMITTED_CORE_MEMBERS, JavaClassMirror, JavaCoreMember, JavaString};
pub use value::{JvmReference, JvmValue, JvmValueWidth, PrimitiveCategory, ReturnCategory};
pub use verifier::{
    ClassMethodProofIdentity, ClassVerificationCache, ClassVerificationError,
    ClassVerificationProof, ExpandedStackMapFrame, FrameError, FrameKind, MethodVerificationError,
    MethodVerificationProof, ReferenceType, StackMapConstraintError, ThrowCapability,
    UnreachableHandlerPolicy, VERIFIER_COVERAGE, VERIFIER_RULES, VerificationAssignability,
    VerificationClass, VerificationConstantResolver, VerificationDependency, VerificationEdgeClass,
    VerificationEdgeId, VerificationEnvironment, VerificationField, VerificationFrame,
    VerificationGraph, VerificationGraphError, VerificationJoinRule, VerificationNodeLocation,
    VerificationQuery, VerificationQueryError, VerificationQueryEvidence, VerificationQueryFailure,
    VerificationState, VerificationTransferError, VerificationTransferKind, VerificationType,
    VerificationTypeJoin, VerificationTypeWidth, VerifierRule, VerifierRuleFamily,
    build_verification_graph, seal_class_verification, seal_method_verification,
    transfer_memory_instruction, transfer_storage_instruction, verifier_admitted_lambda_protocols,
    verifier_rule,
};

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
