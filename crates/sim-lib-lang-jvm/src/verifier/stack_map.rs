/// Located refusal from the constants, locals, and stack transfer family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationTransferError {
    /// Shared instruction identity at the refusal.
    pub instruction: InstructionId,
    /// Exact classfile byte offset at the refusal.
    pub offset: usize,
    /// Opcode being checked.
    pub opcode: Opcode,
    /// Stable refusal classification.
    pub kind: VerificationTransferKind,
}

/// Applies one numeric arithmetic, bitwise, shift, comparison, or conversion rule atomically.
pub fn transfer_numeric_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if opcode == Opcode::Iinc {
        return transfer_storage_instruction(instruction, offset, state, &|_| None);
    }
    if !instruction.instruction().operands.is_empty() {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let (inputs, output): (&[VerificationType], VerificationType) = numeric_signature(opcode)
        .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
    let mut values = stack_values(&state.stack);
    if values.len() < inputs.len() {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let split = values.len() - inputs.len();
    if values[split..] != *inputs {
        return Err(fail(VerificationTransferKind::Category));
    }
    values.truncate(split);
    values.push(output);
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn numeric_signature(opcode: Opcode) -> Option<(&'static [VerificationType], VerificationType)> {
    use Opcode::*;
    use VerificationType::{Double, Float, Int, Long};
    const II: &[VerificationType] = &[Int, Int];
    const LL: &[VerificationType] = &[Long, Long];
    const FF: &[VerificationType] = &[Float, Float];
    const DD: &[VerificationType] = &[Double, Double];
    const LI: &[VerificationType] = &[Long, Int];
    const I: &[VerificationType] = &[Int];
    const L: &[VerificationType] = &[Long];
    const F: &[VerificationType] = &[Float];
    const D: &[VerificationType] = &[Double];
    Some(match opcode {
        Iadd | Isub | Imul | Idiv | Irem | Iand | Ior | Ixor | Ishl | Ishr | Iushr => (II, Int),
        Ladd | Lsub | Lmul | Ldiv | Lrem | Land | Lor | Lxor => (LL, Long),
        Lshl | Lshr | Lushr => (LI, Long),
        Fadd | Fsub | Fmul | Fdiv | Frem => (FF, Float),
        Dadd | Dsub | Dmul | Ddiv | Drem => (DD, Double),
        Ineg | I2b | I2c | I2s => (I, Int),
        Lneg => (L, Long),
        Fneg => (F, Float),
        Dneg => (D, Double),
        I2l => (I, Long),
        I2f => (I, Float),
        I2d => (I, Double),
        L2i => (L, Int),
        L2f => (L, Float),
        L2d => (L, Double),
        F2i => (F, Int),
        F2l => (F, Long),
        F2d => (F, Double),
        D2i => (D, Int),
        D2l => (D, Long),
        D2f => (D, Float),
        Lcmp => (LL, Int),
        Fcmpl | Fcmpg => (FF, Int),
        Dcmpl | Dcmpg => (DD, Int),
        _ => return None,
    })
}

/// Method facts needed to derive the verifier's implicit entry frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialFrameInput<'a> {
    /// Internal binary name of the class declaring the method.
    pub declaring_class: &'a str,
    /// Exact JVMS method name (`<init>` identifies a constructor).
    pub method_name: &'a str,
    /// Exact JVMS method descriptor.
    pub descriptor: &'a str,
    /// Whether the method carries `ACC_STATIC`.
    pub is_static: bool,
    /// Physical local-variable slot limit from the enclosing `Code` attribute.
    pub max_locals: usize,
    /// Physical operand-stack slot limit from the enclosing `Code` attribute.
    pub max_stack: usize,
}

/// One declared stack-map frame, expanded and anchored to a decoded instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpandedStackMapFrame {
    /// Exact classfile byte offset computed from the compressed deltas.
    pub offset: u32,
    /// Stable identity of the instruction beginning at `offset`.
    pub instruction: InstructionId,
    /// Complete logical local sequence (category-2 tails are implicit).
    pub locals: Box<[VerificationType]>,
    /// Complete logical operand-stack sequence (category-2 tails are implicit).
    pub stack: Box<[VerificationType]>,
}

/// A precise refusal while deriving or expanding declared verifier frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackMapExpansionError {
    /// The method descriptor is malformed.
    InvalidDescriptor {
        /// Refused descriptor.
        descriptor: Box<str>,
        /// Byte offset at which parsing failed.
        offset: usize,
    },
    /// Descriptor-derived locals exceed the `Code` attribute limit.
    LocalsWidth {
        /// Declared frame offset, or `None` for the implicit entry frame.
        offset: Option<u32>,
        /// Required physical slot width.
        width: usize,
        /// Declared `max_locals` limit.
        limit: usize,
    },
    /// A declared operand stack exceeds the `Code` attribute limit.
    StackWidth {
        /// Declared frame offset.
        offset: u32,
        /// Required physical slot width.
        width: usize,
        /// Declared `max_stack` limit.
        limit: usize,
    },
    /// A compressed frame removes more logical locals than its predecessor has.
    ChopUnderflow {
        /// Declared frame offset.
        offset: u32,
        /// Number of logical locals requested for removal.
        requested: usize,
        /// Number of logical predecessor locals available.
        available: usize,
    },
    /// The encoded append count disagrees with its retained payload.
    AppendCount {
        /// Declared frame offset.
        offset: u32,
        /// Count encoded by the frame tag.
        declared: usize,
        /// Number of retained payload entries.
        actual: usize,
    },
    /// A classfile verification type names an invalid constant-pool class entry.
    InvalidClass {
        /// Declared frame offset.
        offset: u32,
        /// Refused constant-pool index.
        constant_pool_index: u16,
    },
    /// Offset-delta progression overflowed the classfile offset domain.
    OffsetOverflow,
    /// The declared offset does not begin a decoded instruction.
    NotInstructionBoundary {
        /// Exact non-boundary byte offset.
        offset: u32,
    },
}

/// Derives the exact logical locals of the implicit entry frame.
pub fn derive_initial_locals(
    input: &InitialFrameInput<'_>,
) -> Result<Vec<VerificationType>, StackMapExpansionError> {
    let mut locals = Vec::new();
    if !input.is_static {
        locals.push(if input.method_name == "<init>" {
            VerificationType::UninitializedThis
        } else {
            VerificationType::Reference(ReferenceType::Class(input.declaring_class.into()))
        });
    }
    let bytes = input.descriptor.as_bytes();
    if !input.descriptor.is_ascii() || bytes.first() != Some(&b'(') {
        return Err(descriptor_error(input.descriptor, 0));
    }
    let mut cursor = 1;
    while bytes.get(cursor) != Some(&b')') {
        if cursor >= bytes.len() {
            return Err(descriptor_error(input.descriptor, cursor));
        }
        locals.push(parse_descriptor_type(input.descriptor, &mut cursor)?);
    }
    cursor += 1;
    if cursor >= bytes.len() {
        return Err(descriptor_error(input.descriptor, cursor));
    }
    if bytes[cursor] == b'V' {
        cursor += 1;
    } else {
        let _ = parse_descriptor_type(input.descriptor, &mut cursor)?;
    }
    if cursor != bytes.len() {
        return Err(descriptor_error(input.descriptor, cursor));
    }
    let width = verification_width(&locals);
    if width > input.max_locals {
        return Err(StackMapExpansionError::LocalsWidth {
            offset: None,
            width,
            limit: input.max_locals,
        });
    }
    Ok(locals)
}

/// Expands verbatim classfile stack-map records and binds each to an instruction identity.
pub fn expand_stack_map_table<F>(
    table: &StackMapTableAttribute,
    input: &InitialFrameInput<'_>,
    code: &LocatedCode<PreparedJvmPolicy>,
    mut resolve_class: F,
) -> Result<Vec<ExpandedStackMapFrame>, StackMapExpansionError>
where
    F: FnMut(u16) -> Option<ReferenceType>,
{
    let mut locals = derive_initial_locals(input)?;
    let mut previous_offset: Option<u32> = None;
    let mut expanded = Vec::with_capacity(table.frames.len());
    for encoded in &table.frames {
        let delta = frame_offset_delta(encoded);
        let offset = match previous_offset {
            None => u32::from(delta),
            Some(previous) => previous
                .checked_add(u32::from(delta))
                .and_then(|value| value.checked_add(1))
                .ok_or(StackMapExpansionError::OffsetOverflow)?,
        };
        let mut stack = Vec::new();
        match encoded {
            StackMapFrame::Same { .. } | StackMapFrame::SameExtended { .. } => {}
            StackMapFrame::SameLocalsOneStack { stack: value, .. }
            | StackMapFrame::SameLocalsOneStackExtended { stack: value, .. } => {
                stack.push(convert_classfile_type(*value, offset, &mut resolve_class)?);
            }
            StackMapFrame::Chop { frame_type, .. } => {
                let count = usize::from(251 - *frame_type);
                if count > locals.len() {
                    return Err(StackMapExpansionError::ChopUnderflow {
                        offset,
                        requested: count,
                        available: locals.len(),
                    });
                }
                locals.truncate(locals.len() - count);
            }
            StackMapFrame::Append {
                frame_type,
                locals: appended,
                ..
            } => {
                let declared = usize::from(*frame_type - 251);
                if declared != appended.len() {
                    return Err(StackMapExpansionError::AppendCount {
                        offset,
                        declared,
                        actual: appended.len(),
                    });
                }
                for value in appended {
                    locals.push(convert_classfile_type(*value, offset, &mut resolve_class)?);
                }
            }
            StackMapFrame::Full {
                locals: complete,
                stack: complete_stack,
                ..
            } => {
                locals = complete
                    .iter()
                    .map(|value| convert_classfile_type(*value, offset, &mut resolve_class))
                    .collect::<Result<_, _>>()?;
                stack = complete_stack
                    .iter()
                    .map(|value| convert_classfile_type(*value, offset, &mut resolve_class))
                    .collect::<Result<_, _>>()?;
            }
        }
        let locals_width = verification_width(&locals);
        if locals_width > input.max_locals {
            return Err(StackMapExpansionError::LocalsWidth {
                offset: Some(offset),
                width: locals_width,
                limit: input.max_locals,
            });
        }
        let stack_width = verification_width(&stack);
        if stack_width > input.max_stack {
            return Err(StackMapExpansionError::StackWidth {
                offset,
                width: stack_width,
                limit: input.max_stack,
            });
        }
        let instruction = instruction_at_offset(code, offset)
            .ok_or(StackMapExpansionError::NotInstructionBoundary { offset })?;
        expanded.push(ExpandedStackMapFrame {
            offset,
            instruction,
            locals: locals.clone().into_boxed_slice(),
            stack: stack.into_boxed_slice(),
        });
        previous_offset = Some(offset);
    }
    Ok(expanded)
}

fn descriptor_error(descriptor: &str, offset: usize) -> StackMapExpansionError {
    StackMapExpansionError::InvalidDescriptor {
        descriptor: descriptor.into(),
        offset,
    }
}

fn parse_descriptor_type(
    descriptor: &str,
    cursor: &mut usize,
) -> Result<VerificationType, StackMapExpansionError> {
    let bytes = descriptor.as_bytes();
    let start = *cursor;
    let value = match bytes.get(*cursor).copied() {
        Some(b'B' | b'C' | b'I' | b'S' | b'Z') => VerificationType::Int,
        Some(b'F') => VerificationType::Float,
        Some(b'J') => VerificationType::Long,
        Some(b'D') => VerificationType::Double,
        Some(b'L') => {
            let end = bytes[*cursor + 1..]
                .iter()
                .position(|byte| *byte == b';')
                .map(|relative| *cursor + 1 + relative)
                .ok_or_else(|| descriptor_error(descriptor, start))?;
            if end == *cursor + 1 {
                return Err(descriptor_error(descriptor, start));
            }
            let name = &descriptor[*cursor + 1..end];
            *cursor = end;
            VerificationType::Reference(ReferenceType::Class(name.into()))
        }
        Some(b'[') => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            match bytes.get(*cursor).copied() {
                Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {}
                Some(b'L') => {
                    let end = bytes[*cursor + 1..]
                        .iter()
                        .position(|byte| *byte == b';')
                        .map(|relative| *cursor + 1 + relative)
                        .ok_or_else(|| descriptor_error(descriptor, start))?;
                    if end == *cursor + 1 {
                        return Err(descriptor_error(descriptor, start));
                    }
                    *cursor = end;
                }
                _ => return Err(descriptor_error(descriptor, start)),
            }
            VerificationType::Reference(ReferenceType::Array(descriptor[start..=*cursor].into()))
        }
        _ => return Err(descriptor_error(descriptor, start)),
    };
    *cursor += 1;
    Ok(value)
}

fn frame_offset_delta(frame: &StackMapFrame) -> u16 {
    match frame {
        StackMapFrame::Same { frame_type } => u16::from(*frame_type),
        StackMapFrame::SameLocalsOneStack { frame_type, .. } => u16::from(*frame_type - 64),
        StackMapFrame::SameLocalsOneStackExtended { offset_delta, .. }
        | StackMapFrame::Chop { offset_delta, .. }
        | StackMapFrame::SameExtended { offset_delta }
        | StackMapFrame::Append { offset_delta, .. }
        | StackMapFrame::Full { offset_delta, .. } => *offset_delta,
    }
}

fn convert_classfile_type<F>(
    value: ClassfileVerificationType,
    offset: u32,
    resolve_class: &mut F,
) -> Result<VerificationType, StackMapExpansionError>
where
    F: FnMut(u16) -> Option<ReferenceType>,
{
    Ok(match value {
        ClassfileVerificationType::Top => VerificationType::Unusable,
        ClassfileVerificationType::Integer => VerificationType::Int,
        ClassfileVerificationType::Float => VerificationType::Float,
        ClassfileVerificationType::Double => VerificationType::Double,
        ClassfileVerificationType::Long => VerificationType::Long,
        ClassfileVerificationType::Null => VerificationType::Null,
        ClassfileVerificationType::UninitializedThis => VerificationType::UninitializedThis,
        ClassfileVerificationType::Object(index) => resolve_class(index)
            .map(VerificationType::Reference)
            .ok_or(StackMapExpansionError::InvalidClass {
                offset,
                constant_pool_index: index,
            })?,
        ClassfileVerificationType::Uninitialized(new_offset) => {
            VerificationType::Uninitialized(u32::from(new_offset))
        }
    })
}

fn verification_width(values: &[VerificationType]) -> usize {
    values
        .iter()
        .map(|value| match value.width() {
            Some(VerificationTypeWidth::Category2) => 2,
            _ => 1,
        })
        .sum()
}

fn instruction_at_offset(
    code: &LocatedCode<PreparedJvmPolicy>,
    wanted: u32,
) -> Option<InstructionId> {
    cursors(code).find_map(|cursor| {
        let instruction = code.instruction(cursor);
        let (offset, _) = byte_range(instruction.location());
        (offset == wanted as usize).then_some(*instruction.id())
    })
}
