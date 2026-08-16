//! Manifest-driven preparation of decoded JVM bytecode for the shared machine.

use sim_codec_classfile::{
    CodeException, DecodedCode, ExceptionHandlerRange, Instruction, InstructionError,
    InstructionErrorKind, InstructionId, InstructionOperand, Opcode, validate_exception_handlers,
};
use sim_incremental_core::ValueFingerprint;
use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_machine::{
    BranchTarget, CodeError, InstructionPolicy, LocatedCode, LocatedInstruction, SourceLocation,
    TargetLocation,
};

use crate::verifier::{PREPARED_DISPATCH, PreparedDispatchFamily};
use crate::{
    ClassDefinitionId, ClassSpaceRevision, ClassVerificationProof, ReferenceType,
    VerificationFrame, VerificationState, VerificationType,
};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("code/model.rs");
include!("code/prepare.rs");
include!("code/tests.rs");
