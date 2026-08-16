//! Manifest-driven preparation of decoded JVM bytecode for the shared machine.

use sim_codec_classfile::{
    CodeException, DecodedCode, ExceptionHandlerRange, Instruction, InstructionError,
    InstructionId, InstructionOperand, Opcode, validate_exception_handlers,
};
use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_machine::{
    BranchTarget, CodeError, InstructionPolicy, LocatedCode, LocatedInstruction, SourceLocation,
    TargetLocation,
};

/// The JVM storage category consumed or produced by an instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JvmSlotKind {
    /// A category-one non-reference value.
    CategoryOne,
    /// A category-two value occupying two JVM slots.
    CategoryTwo,
    /// A managed JVM reference and therefore a potential GC root.
    Reference,
}

impl JvmSlotKind {
    /// Returns the JVM slot width of this value category.
    pub const fn width(self) -> usize {
        match self {
            Self::CategoryOne | Self::Reference => 1,
            Self::CategoryTwo => 2,
        }
    }
}

/// Root-set change caused by one instruction's operand-stack effects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RootEffect {
    /// Reference operands removed from the stack.
    pub removed: usize,
    /// Reference results added to the stack.
    pub added: usize,
}

/// One classfile exception-table row resolved entirely to prepared instruction identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedExceptionHandler {
    /// Original exception-table row, preserving classfile search order.
    pub row: usize,
    /// First protected instruction, inclusive.
    pub start: InstructionId,
    /// First instruction after the protected range, or `None` at code end.
    pub end: Option<InstructionId>,
    /// Prepared handler entry instruction.
    pub handler: InstructionId,
    /// Constant-pool catch class, or zero for a catch-all handler.
    pub catch_type: u16,
}

/// Complete execution metadata required before a JVM instruction can run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JvmInstructionSemantics {
    /// Operand categories popped in JVM evaluation order.
    pub pops: &'static [JvmSlotKind],
    /// Result categories pushed in JVM evaluation order.
    pub pushes: &'static [JvmSlotKind],
    /// Whether execution must poll managed-runtime state before this instruction.
    pub safepoint: bool,
}

impl JvmInstructionSemantics {
    /// Returns the total input width in JVM slots.
    pub fn input_width(self) -> usize {
        self.pops.iter().copied().map(JvmSlotKind::width).sum()
    }

    /// Returns the total output width in JVM slots.
    pub fn output_width(self) -> usize {
        self.pushes.iter().copied().map(JvmSlotKind::width).sum()
    }

    /// Derives the managed-root change from the declared value categories.
    pub fn root_effect(self) -> RootEffect {
        RootEffect {
            removed: self
                .pops
                .iter()
                .filter(|kind| **kind == JvmSlotKind::Reference)
                .count(),
            added: self
                .pushes
                .iter()
                .filter(|kind| **kind == JvmSlotKind::Reference)
                .count(),
        }
    }
}

/// Supplies executable semantics for manifest-admitted classfile opcodes.
///
/// This is deliberately a coverage policy rather than a second opcode table. Implementations
/// match the shared [`Opcode`] identity and preparation refuses every identity they omit.
pub trait JvmInstructionPolicy {
    /// Returns executable semantics when this runtime implements `opcode`.
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics>;
}

/// A decoded JVM instruction with all execution metadata frozen beside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedJvmInstruction {
    id: InstructionId,
    opcode: Opcode,
    instruction: Instruction,
    input_width: usize,
    output_width: usize,
    root_effect: RootEffect,
    handler_membership: Box<[PreparedExceptionHandler]>,
    handler_entries: Box<[usize]>,
}

impl PreparedJvmInstruction {
    /// Returns the stream-local instruction identity.
    pub const fn id(&self) -> InstructionId {
        self.id
    }

    /// Returns the shared, generated classfile opcode identity.
    pub const fn opcode(&self) -> Opcode {
        self.opcode
    }

    /// Returns the losslessly decoded instruction and operands.
    pub fn instruction(&self) -> &Instruction {
        &self.instruction
    }

    /// Returns the operand width consumed in JVM slots.
    pub const fn input_width(&self) -> usize {
        self.input_width
    }

    /// Returns the result width produced in JVM slots.
    pub const fn output_width(&self) -> usize {
        self.output_width
    }

    /// Returns the instruction's managed-root stack effect.
    pub const fn root_effect(&self) -> RootEffect {
        self.root_effect
    }

    /// Returns exception-table rows whose protected range contains this instruction.
    pub fn handler_membership(&self) -> &[PreparedExceptionHandler] {
        &self.handler_membership
    }

    /// Returns exception-table rows that enter at this instruction.
    pub fn handler_entries(&self) -> &[usize] {
        &self.handler_entries
    }
}

/// Shared-machine identity policy for prepared JVM instructions.
pub struct PreparedJvmPolicy;

impl InstructionPolicy for PreparedJvmPolicy {
    type Instruction = PreparedJvmInstruction;
    type InstructionId = InstructionId;

    fn instruction_id(instruction: &Self::Instruction) -> Self::InstructionId {
        instruction.id
    }
}

/// Typed refusal from JVM code preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparationError {
    /// Classfile control metadata is not aligned to decoded instructions.
    Classfile(InstructionError),
    /// The runtime has no execution policy for a manifest-admitted opcode.
    MissingInstructionPolicy {
        /// Shared opcode identity lacking policy.
        opcode: Opcode,
        /// Manifest mnemonic for diagnostics.
        mnemonic: &'static str,
        /// Exact classfile byte offset of the instruction.
        offset: u32,
    },
    /// A branch displacement overflowed its classfile byte offset.
    BranchOffsetOverflow {
        /// Instruction containing the branch.
        instruction: InstructionId,
        /// Exact classfile byte offset of the instruction.
        offset: u32,
        /// Signed relative displacement.
        displacement: i32,
    },
    /// Shared machine validation rejected the prepared code.
    Machine(CodeError<InstructionId>),
}

/// Lowers decoded classfile code into immutable, byte-located shared-machine code.
pub fn prepare_code<P: JvmInstructionPolicy>(
    decoded: &DecodedCode,
    code_length: usize,
    exception_table: &[CodeException],
    source: SourceId,
) -> Result<LocatedCode<PreparedJvmPolicy>, PreparationError> {
    let ranges: Vec<_> = exception_table
        .iter()
        .map(|entry| ExceptionHandlerRange {
            start: entry.start_pc,
            end: entry.end_pc,
            handler: entry.handler_pc,
        })
        .collect();
    validate_exception_handlers(decoded, code_length, &ranges)
        .map_err(PreparationError::Classfile)?;

    let mut instructions = Vec::with_capacity(decoded.instructions.len());
    let mut targets = Vec::new();
    for (index, located) in decoded.instructions.iter().enumerate() {
        let opcode = located.instruction.opcode;
        let semantics = P::semantics(opcode).ok_or(PreparationError::MissingInstructionPolicy {
            opcode,
            mnemonic: opcode.metadata().mnemonic,
            offset: located.offset,
        })?;
        let end = decoded
            .instructions
            .get(index + 1)
            .map_or(code_length, |next| next.offset as usize);
        let membership = exception_table
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                u32::from(entry.start_pc) <= located.offset
                    && located.offset < u32::from(entry.end_pc)
            })
            .map(|(row, entry)| PreparedExceptionHandler {
                row,
                start: decoded.offsets[&u32::from(entry.start_pc)],
                end: decoded.offsets.get(&u32::from(entry.end_pc)).copied(),
                handler: decoded.offsets[&u32::from(entry.handler_pc)],
                catch_type: entry.catch_type,
            })
            .collect::<Vec<_>>();
        let entries = exception_table
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| {
                (u32::from(entry.handler_pc) == located.offset).then_some(row)
            })
            .collect::<Vec<_>>();
        let prepared = PreparedJvmInstruction {
            id: located.id,
            opcode,
            instruction: located.instruction.clone(),
            input_width: semantics.input_width(),
            output_width: semantics.output_width(),
            root_effect: semantics.root_effect(),
            handler_membership: membership.into_boxed_slice(),
            handler_entries: entries.into_boxed_slice(),
        };
        instructions.push(LocatedInstruction::new(
            prepared,
            located.id,
            SourceLocation::Bytes(Origin {
                codec: CodecId(0),
                source: source.clone(),
                span: Span {
                    start: located.offset as usize,
                    end,
                },
                trivia: Vec::new(),
            }),
            semantics.safepoint,
            None,
        ));
        for operand in &located.instruction.operands {
            if let InstructionOperand::Branch(displacement) = operand {
                let target = i64::from(located.offset) + i64::from(*displacement);
                let offset = usize::try_from(target).map_err(|_| {
                    PreparationError::BranchOffsetOverflow {
                        instruction: located.id,
                        offset: located.offset,
                        displacement: *displacement,
                    }
                })?;
                targets.push(BranchTarget {
                    from: located.id,
                    to: TargetLocation::Byte {
                        source: source.clone(),
                        offset,
                    },
                });
            }
        }
    }
    // JVM handlers are ordered and may share ranges, unlike the machine's single-handler
    // nested-region abstraction. The lossless resolved table therefore lives on each protected
    // instruction and is interpreted by JVM abrupt-completion policy.
    LocatedCode::freeze(instructions, targets, Vec::new()).map_err(PreparationError::Machine)
}
