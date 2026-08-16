/// Resolved symbolic facts for one constructor invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationConstructor {
    /// Internal class name of the constructed object.
    pub owner: Box<str>,
    /// Exact JVM member name; legal initialization requires `<init>`.
    pub name: Box<str>,
    /// Exact JVM method descriptor.
    pub descriptor: Box<str>,
    /// Exact allocation-site type legally consumed by this resolved constructor.
    pub receiver: VerificationType,
}
/// Applies `new`, retaining the prepared instruction identity as the allocation-site type.
pub fn transfer_new_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode: instruction.opcode(),
        kind,
    };
    if instruction.opcode() != Opcode::New
        || !matches!(
            instruction.instruction().operands.as_slice(),
            [InstructionOperand::Constant(_)]
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let mut next = state.clone();
    next.stack
        .push(VerificationType::Uninitialized(instruction.id().0))
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

/// Applies `invokespecial <init>`, replacing every frame alias after successful initialization.
pub fn transfer_constructor_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    constructor: &VerificationConstructor,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode: instruction.opcode(),
        kind,
    };
    if instruction.opcode() != Opcode::Invokespecial
        || !matches!(
            instruction.instruction().operands.as_slice(),
            [InstructionOperand::Constant(_)]
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    if constructor.name.as_ref() != "<init>" {
        return Err(fail(VerificationTransferKind::IllegalConstructorReceiver));
    }
    let arguments = descriptor_arguments(&constructor.descriptor)
        .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
    let mut values = stack_values(&state.stack);
    if values.len() < arguments.len() + 1 {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let receiver_index = values.len() - arguments.len() - 1;
    if !values[receiver_index + 1..]
        .iter()
        .zip(&arguments)
        .all(|(actual, expected)| verification_category_matches(actual, expected))
    {
        return Err(fail(VerificationTransferKind::Category));
    }
    let receiver = values[receiver_index].clone();
    if receiver != constructor.receiver
        || !matches!(
            receiver,
            VerificationType::Uninitialized(_) | VerificationType::UninitializedThis
        )
    {
        return Err(fail(VerificationTransferKind::IllegalConstructorReceiver));
    }
    let initialized = VerificationType::Reference(ReferenceType::Class(constructor.owner.clone()));
    values.truncate(receiver_index);
    for value in &mut values {
        if *value == receiver {
            *value = initialized.clone();
        }
    }
    let mut next = state.clone();
    replace_alias(&mut next.locals, &receiver, &initialized);
    replace_alias(&mut next.stack, &receiver, &initialized);
    next.stack = stack_from_values(next.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

/// Rejects a control-flow merge that combines initialized state with a live allocation alias.
pub fn join_initialization_states(
    instruction: InstructionId,
    offset: usize,
    left: &VerificationState,
    right: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = || VerificationTransferError {
        instruction,
        offset,
        opcode: Opcode::Nop,
        kind: VerificationTransferKind::InitializationMerge,
    };
    reject_initialization_conflict(&left.locals, &right.locals)
        .then_some(())
        .ok_or_else(fail)?;
    reject_initialization_conflict(&left.stack, &right.stack)
        .then_some(())
        .ok_or_else(fail)?;
    Ok(VerificationState {
        locals: left.locals.join(&right.locals),
        stack: left.stack.join(&right.stack),
    })
}

/// Builds a handler-entry state only when no pre-initialization alias is live.
pub fn handler_entry_state(
    instruction: InstructionId,
    offset: usize,
    state: &VerificationState,
    exception: ReferenceType,
) -> Result<VerificationState, VerificationTransferError> {
    if frame_has_uninitialized(&state.locals) || frame_has_uninitialized(&state.stack) {
        return Err(VerificationTransferError {
            instruction,
            offset,
            opcode: Opcode::Athrow,
            kind: VerificationTransferKind::UninitializedHandlerEntry,
        });
    }
    let mut stack = VerificationFrame::new(FrameKind::OperandStack, state.stack.capacity());
    stack
        .push(VerificationType::Reference(exception))
        .map_err(|_| VerificationTransferError {
            instruction,
            offset,
            opcode: Opcode::Athrow,
            kind: VerificationTransferKind::StackBounds,
        })?;
    Ok(VerificationState {
        locals: state.locals.clone(),
        stack,
    })
}

fn descriptor_arguments(descriptor: &str) -> Option<Vec<VerificationType>> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut cursor = 1;
    let mut arguments = Vec::new();
    while bytes.get(cursor) != Some(&b')') {
        arguments.push(parse_descriptor_type(descriptor, &mut cursor).ok()?);
    }
    Some(arguments)
}

fn replace_alias(frame: &mut VerificationFrame, from: &VerificationType, to: &VerificationType) {
    if let VerificationFrame::Reachable { slots, .. } = frame {
        for slot in slots.iter_mut() {
            if matches!(slot, Slot::Value(value) if value == from) {
                *slot = Slot::Value(to.clone());
            }
        }
    }
}

fn frame_has_uninitialized(frame: &VerificationFrame) -> bool {
    frame.normalized_slots().is_some_and(|slots| {
        slots.iter().any(|slot| {
            matches!(
                slot,
                Slot::Value(
                    VerificationType::Uninitialized(_) | VerificationType::UninitializedThis
                )
            )
        })
    })
}

fn reject_initialization_conflict(left: &VerificationFrame, right: &VerificationFrame) -> bool {
    match (left.normalized_slots(), right.normalized_slots()) {
        (Some(left), Some(right)) => {
            left.iter()
                .zip(right)
                .all(|(left, right)| match (left, right) {
                    (Slot::Value(a), Slot::Value(b)) => {
                        let a_uninit = matches!(
                            a,
                            VerificationType::Uninitialized(_)
                                | VerificationType::UninitializedThis
                        );
                        let b_uninit = matches!(
                            b,
                            VerificationType::Uninitialized(_)
                                | VerificationType::UninitializedThis
                        );
                        a_uninit == b_uninit && (!a_uninit || a == b)
                    }
                    _ => true,
                })
        }
        _ => true,
    }
}

/// Descriptor-derived return category used by the control verifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationReturnType {
    /// The method returns no value.
    Void,
    /// The method returns this verification type.
    Value(VerificationType),
}

/// Applies one branch, switch, or return rule without selecting an edge.
///
/// Edge selection remains the graph's concern: both conditional successors receive the same
/// post-pop state, switches propagate to every declared target, and returns propagate nowhere.
pub fn transfer_control_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    return_type: &VerificationReturnType,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let operands = instruction.instruction().operands.as_slice();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let expected = match opcode {
        Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle | Tableswitch | Lookupswitch => {
            &[VerificationType::Int][..]
        }
        IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge | IfIcmpgt | IfIcmple => {
            &[VerificationType::Int, VerificationType::Int][..]
        }
        IfAcmpeq | IfAcmpne => &[
            VerificationType::Reference(ReferenceType::Object),
            VerificationType::Reference(ReferenceType::Object),
        ][..],
        Ifnull | Ifnonnull => &[VerificationType::Reference(ReferenceType::Object)][..],
        Goto | GotoW => &[][..],
        Ireturn => &[VerificationType::Int][..],
        Lreturn => &[VerificationType::Long][..],
        Freturn => &[VerificationType::Float][..],
        Dreturn => &[VerificationType::Double][..],
        Areturn => &[VerificationType::Reference(ReferenceType::Object)][..],
        Return => &[][..],
        _ => return Err(fail(VerificationTransferKind::MalformedPreparedInput)),
    };
    if !control_operands_valid(opcode, operands) {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let values = stack_values(&state.stack);
    if values.len() < expected.len() {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let split = values.len() - expected.len();
    if !values[split..]
        .iter()
        .zip(expected)
        .all(|(actual, wanted)| verification_category_matches(actual, wanted))
    {
        return Err(fail(VerificationTransferKind::Category));
    }
    let actual_return = match opcode {
        Ireturn | Lreturn | Freturn | Dreturn | Areturn => {
            Some(values.last().expect("return input was checked"))
        }
        Return => None,
        _ => {
            let mut next = state.clone();
            next.stack = stack_from_values(state.stack.capacity(), values[..split].to_vec())
                .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
            return Ok(next);
        }
    };
    if frame_has_uninitialized(&state.locals) || frame_has_uninitialized(&state.stack) {
        return Err(fail(VerificationTransferKind::UninitializedUse));
    }
    let compatible = match (actual_return, return_type) {
        (None, VerificationReturnType::Void) => true,
        (Some(actual), VerificationReturnType::Value(declared)) => actual.less_equal(declared),
        _ => false,
    };
    if !compatible {
        return Err(fail(VerificationTransferKind::ReturnType));
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values[..split].to_vec())
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn verification_category_matches(actual: &VerificationType, wanted: &VerificationType) -> bool {
    match wanted {
        VerificationType::Reference(_) => matches!(
            actual,
            VerificationType::Null | VerificationType::Reference(_)
        ),
        _ => actual == wanted,
    }
}

fn control_operands_valid(opcode: Opcode, operands: &[InstructionOperand]) -> bool {
    use Opcode::*;
    match opcode {
        Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle | IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge
        | IfIcmpgt | IfIcmple | IfAcmpeq | IfAcmpne | Goto | Ifnull | Ifnonnull | GotoW => {
            matches!(operands, [InstructionOperand::Branch(_)])
        }
        Tableswitch => matches!(
            operands,
            [InstructionOperand::Branch(_), InstructionOperand::TableLow(low), InstructionOperand::TableHigh(high), rest @ ..]
            if i64::from(*high) - i64::from(*low) + 1 == rest.len() as i64
                && rest.iter().all(|operand| matches!(operand, InstructionOperand::Branch(_)))
        ),
        Lookupswitch => {
            matches!(operands.split_first(), Some((InstructionOperand::Branch(_), rest))
            if rest.len() % 2 == 0 && rest.chunks_exact(2).all(|pair| matches!(pair,
                [InstructionOperand::LookupKey(_), InstructionOperand::Branch(_)])))
        }
        Ireturn | Lreturn | Freturn | Dreturn | Areturn | Return => operands.is_empty(),
        _ => false,
    }
}
