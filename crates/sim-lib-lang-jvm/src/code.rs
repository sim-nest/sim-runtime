//! Manifest-driven preparation of decoded JVM bytecode for the shared machine.

use sim_codec_classfile::{
    CodeException, DecodedCode, ExceptionHandlerRange, Instruction, InstructionError,
    InstructionErrorKind, InstructionId, InstructionOperand, Opcode, validate_exception_handlers,
};
use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_machine::{
    BranchTarget, CodeError, InstructionPolicy, LocatedCode, LocatedInstruction, SourceLocation,
    TargetLocation,
};

use crate::ClassSpaceRevision;
use crate::verifier::{PREPARED_DISPATCH, PreparedDispatchFamily};

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

/// Identity of prepared method code, invalidated by either bytecode or class-space change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCodeIdentity {
    content: Box<[u8]>,
    revision: ClassSpaceRevision,
}

impl PreparedCodeIdentity {
    /// Creates an identity from the exact Code attribute bytes and observed class-space revision.
    pub fn new(code: &[u8], revision: ClassSpaceRevision) -> Self {
        let content = code.into();
        Self { content, revision }
    }

    /// Returns whether the prepared identity still denotes these exact inputs.
    pub fn matches(&self, code: &[u8], revision: ClassSpaceRevision) -> bool {
        self.content.as_ref() == code && self.revision == revision
    }

    /// Returns the class-space revision captured during preparation.
    pub const fn revision(&self) -> ClassSpaceRevision {
        self.revision
    }
}

/// Operand and control decisions computed once before shared-machine execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedJvmOperands {
    /// No decoded operand is needed by execution.
    None,
    /// A signed immediate constant.
    Immediate(i32),
    /// A constant-pool, field, method, class, or bootstrap site key.
    ConstantSite(u16),
    /// A local slot already widened to the machine's index type.
    Local(usize),
    /// A widened local slot and signed increment.
    Increment {
        /// Widened local slot.
        slot: usize,
        /// Signed wrapping increment.
        amount: i32,
    },
    /// One direct control target.
    Direct(InstructionId),
    /// Dense switch search structure, including its default target.
    Table {
        /// Lowest key in the dense range.
        low: i32,
        /// Target selected outside the dense range.
        default: InstructionId,
        /// Targets indexed by `key - low`.
        targets: Box<[InstructionId]>,
    },
    /// Sorted sparse switch search structure, including its default target.
    Lookup {
        /// Target selected when binary search misses.
        default: InstructionId,
        /// Sorted key-to-target pairs.
        pairs: Box<[(i32, InstructionId)]>,
    },
    /// Immutable admissible whole-value shuffle layouts.
    Shuffle(&'static [(&'static [usize], &'static [usize])]),
    /// Decoded operands retained for instruction families not yet executed by this module.
    Other(Box<[InstructionOperand]>),
}

/// One classfile exception-table row resolved entirely to prepared instruction identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedCatchEntry {
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
    dispatch: PreparedDispatchFamily,
    instruction: Instruction,
    input_width: usize,
    output_width: usize,
    root_effect: RootEffect,
    operands: PreparedJvmOperands,
    work_charge: usize,
    code_identity: Option<PreparedCodeIdentity>,
    handler_membership: Box<[PreparedCatchEntry]>,
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

    /// Returns the dense execution family frozen during preparation.
    pub const fn dispatch_family(&self) -> PreparedDispatchFamily {
        self.dispatch
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

    /// Returns the fully lowered operands and control search structure.
    pub fn prepared_operands(&self) -> &PreparedJvmOperands {
        &self.operands
    }

    /// Returns the immutable work charged after successful execution.
    pub const fn work_charge(&self) -> usize {
        self.work_charge
    }

    /// Returns the exact classfile-content and class-space identity, when supplied by the loader.
    pub fn code_identity(&self) -> Option<&PreparedCodeIdentity> {
        self.code_identity.as_ref()
    }

    /// Returns exception-table rows whose protected range contains this instruction.
    pub fn handler_membership(&self) -> &[PreparedCatchEntry] {
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
    /// The generated runtime manifest marks this opcode as a preparation-time refusal.
    UnsupportedOpcode {
        /// Shared opcode identity refused before execution.
        opcode: Opcode,
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
    prepare_code_inner::<P>(decoded, code_length, exception_table, source, None)
}

/// Lowers code while binding every prepared instruction to exact bytes and class-space revision.
pub fn prepare_code_bound<P: JvmInstructionPolicy>(
    decoded: &DecodedCode,
    code: &[u8],
    exception_table: &[CodeException],
    source: SourceId,
    revision: ClassSpaceRevision,
) -> Result<LocatedCode<PreparedJvmPolicy>, PreparationError> {
    prepare_code_inner::<P>(
        decoded,
        code.len(),
        exception_table,
        source,
        Some(PreparedCodeIdentity::new(code, revision)),
    )
}

fn prepare_code_inner<P: JvmInstructionPolicy>(
    decoded: &DecodedCode,
    code_length: usize,
    exception_table: &[CodeException],
    source: SourceId,
    code_identity: Option<PreparedCodeIdentity>,
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
        let dispatch = PREPARED_DISPATCH[opcode as u8 as usize].ok_or(
            PreparationError::UnsupportedOpcode {
                opcode,
                offset: located.offset,
            },
        )?;
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
            .map(|(row, entry)| PreparedCatchEntry {
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
        let operands = prepare_operands(decoded, located)?;
        let prepared = PreparedJvmInstruction {
            id: located.id,
            opcode,
            dispatch,
            instruction: located.instruction.clone(),
            input_width: semantics.input_width(),
            output_width: semantics.output_width(),
            root_effect: semantics.root_effect(),
            operands,
            work_charge: 1,
            code_identity: code_identity.clone(),
            handler_membership: membership.into_boxed_slice(),
            handler_entries: entries.into_boxed_slice(),
        };
        let has_backward_edge = located.instruction.operands.iter().any(
            |operand| matches!(operand, InstructionOperand::Branch(displacement) if *displacement < 0),
        );
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
            semantics.safepoint || has_backward_edge,
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

fn prepare_operands(
    decoded: &DecodedCode,
    located: &sim_codec_classfile::LocatedInstruction,
) -> Result<PreparedJvmOperands, PreparationError> {
    let operands = located.instruction.operands.as_slice();
    let target = |displacement: i32| {
        let offset = i64::from(located.offset) + i64::from(displacement);
        let offset = u32::try_from(offset).map_err(|_| PreparationError::BranchOffsetOverflow {
            instruction: located.id,
            offset: located.offset,
            displacement,
        })?;
        decoded.offsets.get(&offset).copied().ok_or_else(|| {
            PreparationError::Classfile(InstructionError {
                kind: InstructionErrorKind::InvalidTarget,
                offset,
                message: "prepared branch target is not an instruction boundary".into(),
            })
        })
    };
    Ok(match operands {
        [] => crate::execution::shuffle_descriptor(located.instruction.opcode)
            .map_or(PreparedJvmOperands::None, PreparedJvmOperands::Shuffle),
        [InstructionOperand::Immediate(value)] => PreparedJvmOperands::Immediate(*value),
        [InstructionOperand::Constant(index)] => PreparedJvmOperands::ConstantSite(*index),
        [InstructionOperand::Local(slot)] => PreparedJvmOperands::Local(usize::from(*slot)),
        [
            InstructionOperand::Local(slot),
            InstructionOperand::Immediate(amount),
        ] => PreparedJvmOperands::Increment {
            slot: usize::from(*slot),
            amount: *amount,
        },
        [InstructionOperand::Branch(displacement)]
            if located.instruction.opcode != Opcode::Lookupswitch =>
        {
            PreparedJvmOperands::Direct(target(*displacement)?)
        }
        [
            InstructionOperand::Branch(default),
            InstructionOperand::TableLow(low),
            InstructionOperand::TableHigh(_),
            rest @ ..,
        ] => PreparedJvmOperands::Table {
            low: *low,
            default: target(*default)?,
            targets: rest
                .iter()
                .map(|operand| match operand {
                    InstructionOperand::Branch(displacement) => target(*displacement),
                    _ => unreachable!("decoder validated tableswitch operands"),
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        },
        [InstructionOperand::Branch(default), rest @ ..]
            if rest.chunks_exact(2).all(|pair| {
                matches!(
                    pair,
                    [
                        InstructionOperand::LookupKey(_),
                        InstructionOperand::Branch(_)
                    ]
                )
            }) =>
        {
            let pairs = rest
                .chunks_exact(2)
                .map(|pair| match pair {
                    [
                        InstructionOperand::LookupKey(key),
                        InstructionOperand::Branch(displacement),
                    ] => Ok((*key, target(*displacement)?)),
                    _ => unreachable!(),
                })
                .collect::<Result<Vec<_>, PreparationError>>()?;
            PreparedJvmOperands::Lookup {
                default: target(*default)?,
                pairs: pairs.into_boxed_slice(),
            }
        }
        _ => PreparedJvmOperands::Other(operands.to_vec().into_boxed_slice()),
    })
}

#[cfg(test)]
mod identity_tests {
    use super::PreparedCodeIdentity;
    use crate::ClassLoader;

    #[test]
    fn class_space_revision_bump_invalidates_prepared_code_identity() {
        let loader = ClassLoader::new(16);
        let bytes = [0x03, 0xac];
        let identity = PreparedCodeIdentity::new(&bytes, loader.revision());
        assert!(identity.matches(&bytes, loader.revision()));
        loader.simulate_class_space_change();
        assert!(!identity.matches(&bytes, loader.revision()));
        assert!(!identity.matches(&[0x04, 0xac], identity.revision()));
    }
}
