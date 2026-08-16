//! JVM verification types and lawful dataflow frames.

use crate::VerifierCoverage;
use sim_codec_classfile::{
    InstructionId, InstructionOperand, Opcode, StackMapFrame, StackMapTableAttribute,
    VerificationType as ClassfileVerificationType,
};
use sim_incremental_core::QueryBudgets;
use sim_incremental_core::dataflow::{
    AdmittedTransfer, Boundary, CompletionProofMismatch, DataflowCompletionProof, DataflowGraph,
    EdgeClass, EdgeSpec, FixpointEngine, GraphBuildError, GraphDirection, JoinSemilattice,
    LocatedGraphAdapter, NodeSpec, StateSize, TransferPolicy,
};
use sim_incremental_core::{FingerprintValue, Observation, Revision, ValueFingerprint};
use sim_lib_machine::{LocatedCode, SourceLocation};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    mem::size_of,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision,
    DynamicBootstrap, DynamicLinkError, JavaClassMetadata, JavaMember, JavaMemberKind, JvmEdge,
    JvmGraphError, JvmHeap, JvmRole, PreparedJvmPolicy, STRING_CONCAT_BOOTSTRAP_DESCRIPTOR,
    STRING_CONCAT_BOOTSTRAP_NAME, STRING_CONCAT_BOOTSTRAP_OWNER,
};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("verifier/graph.rs");
include!("verifier/environment.rs");
include!("verifier/types.rs");
include!("verifier/invocation.rs");
include!("verifier/initialization.rs");
include!("verifier/method_proof.rs");
include!("verifier/class_proof.rs");
include!("verifier/stack_map.rs");
include!("verifier/frame.rs");
include!("verifier/environment_tests.rs");
include!("verifier/tests.rs");
include!("verifier/adversarial_tests.rs");
