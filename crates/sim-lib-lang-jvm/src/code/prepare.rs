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
    prepare_code_inner::<P>(decoded, code_length, exception_table, source, None, None)
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
        None,
    )
}

/// Lowers code and selects verified micro-ops only from an exact current proof and frame fact.
pub fn prepare_code_verified<P: JvmInstructionPolicy>(
    decoded: &DecodedCode,
    code: &[u8],
    exception_table: &[CodeException],
    source: SourceId,
    verification: VerificationPreparation<'_>,
) -> Result<LocatedCode<PreparedJvmPolicy>, PreparationError> {
    let identity = PreparedCodeIdentity::new(code, verification.revision);
    prepare_code_inner::<P>(
        decoded,
        code.len(),
        exception_table,
        source,
        Some(identity),
        Some(&verification),
    )
}

fn prepare_code_inner<P: JvmInstructionPolicy>(
    decoded: &DecodedCode,
    code_length: usize,
    exception_table: &[CodeException],
    source: SourceId,
    code_identity: Option<PreparedCodeIdentity>,
    verification: Option<&VerificationPreparation<'_>>,
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
        let instruction_targets = targets_for_instruction(decoded, located)?;
        let micro_op = select_micro_op(verification, located.id, &instruction_targets, &membership);
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
            micro_op,
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

fn targets_for_instruction(
    decoded: &DecodedCode,
    located: &sim_codec_classfile::LocatedInstruction,
) -> Result<Vec<InstructionId>, PreparationError> {
    located
        .instruction
        .operands
        .iter()
        .filter_map(|operand| match operand {
            InstructionOperand::Branch(displacement) => Some(*displacement),
            _ => None,
        })
        .map(|displacement| {
            let offset = i64::from(located.offset) + i64::from(displacement);
            let offset =
                u32::try_from(offset).map_err(|_| PreparationError::BranchOffsetOverflow {
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
        })
        .collect()
}

fn select_micro_op(
    verification: Option<&VerificationPreparation<'_>>,
    instruction: InstructionId,
    targets: &[InstructionId],
    handlers: &[PreparedCatchEntry],
) -> PreparedMicroOp {
    let Some(verification) = verification else {
        return PreparedMicroOp::Checked;
    };
    let proof = verification.proof;
    let exact = proof.owner() == verification.owner
        && proof.owner_revision() == verification.revision
        && proof.policy_fingerprint() == verification.policy
        && proof.structural_fingerprint() == verification.structural
        && proof.methods().iter().any(|method| {
            method.method() == verification.method && method.proof() == verification.method_proof
        });
    if !exact {
        return PreparedMicroOp::Checked;
    }
    let Some((_, state)) = verification
        .frames
        .iter()
        .find(|(id, _)| *id == instruction)
    else {
        return PreparedMicroOp::Checked;
    };
    lower_guarantee(state, targets, handlers)
        .map_or(PreparedMicroOp::Checked, PreparedMicroOp::Verified)
}

fn lower_guarantee(
    state: &VerificationState,
    targets: &[InstructionId],
    handlers: &[PreparedCatchEntry],
) -> Option<PreparedVerificationGuarantee> {
    fn value(value: &VerificationType) -> Option<PreparedValueGuarantee> {
        Some(match value {
            VerificationType::Int => PreparedValueGuarantee::Int,
            VerificationType::Float => PreparedValueGuarantee::Float,
            VerificationType::Long => PreparedValueGuarantee::Long,
            VerificationType::Double => PreparedValueGuarantee::Double,
            VerificationType::Null => PreparedValueGuarantee::Null,
            VerificationType::Reference(reference) => {
                PreparedValueGuarantee::Reference(reference.clone())
            }
            VerificationType::UninitializedThis | VerificationType::Uninitialized(_) => {
                PreparedValueGuarantee::Uninitialized
            }
            VerificationType::Bottom | VerificationType::Unusable => return None,
        })
    }
    fn entries(frame: &VerificationFrame) -> Option<Vec<(usize, PreparedValueGuarantee)>> {
        if matches!(frame, VerificationFrame::Bottom { .. }) {
            return None;
        }
        (0..frame.capacity())
            .filter_map(|slot| frame.get(slot).map(|v| value(v).map(|v| (slot, v))))
            .collect::<Option<Vec<_>>>()
    }
    let stack_entries = entries(&state.stack)?;
    let local_entries = entries(&state.locals)?;
    let stack_width = stack_entries
        .iter()
        .map(|(_, value)| match value {
            PreparedValueGuarantee::Long | PreparedValueGuarantee::Double => 2,
            _ => 1,
        })
        .sum();
    Some(PreparedVerificationGuarantee {
        stack_width,
        local_width: state.locals.capacity(),
        stack: stack_entries.into_iter().map(|(_, value)| value).collect(),
        locals: local_entries.into_boxed_slice(),
        targets: targets.into(),
        handlers: handlers.into(),
    })
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
