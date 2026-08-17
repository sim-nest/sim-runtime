/// A locals or operand frame suitable for generic fixpoint dataflow.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum VerificationFrame {
    /// No control-flow predecessor has reached this frame.
    Bottom {
        /// Frame role.
        kind: FrameKind,
        /// Fixed slot capacity.
        capacity: usize,
    },
    /// A reachable frame with an explicit slot layout.
    Reachable {
        /// Frame role.
        kind: FrameKind,
        /// Physical JVM slots.
        slots: Box<[VerificationSlot]>,
    },
}

impl VerificationFrame {
    /// Creates an unreachable frame with fixed shape.
    #[must_use]
    pub const fn bottom_frame(kind: FrameKind, capacity: usize) -> Self {
        Self::Bottom { kind, capacity }
    }

    /// Creates a reachable frame whose slots are initially unusable.
    #[must_use]
    pub fn new(kind: FrameKind, capacity: usize) -> Self {
        Self::Reachable {
            kind,
            slots: vec![Slot::Unusable; capacity].into_boxed_slice(),
        }
    }

    /// Performs a fallible, hierarchy-aware frame join.
    pub fn join_with_environment(
        &self,
        other: &Self,
        environment: &VerificationEnvironment<'_>,
        node_limit: usize,
    ) -> Result<VerificationQuery<Self>, VerificationQueryFailure> {
        let start = environment.dependencies.borrow().len();
        let mut remaining = node_limit;
        if self.kind() != other.kind() || self.capacity() != other.capacity() {
            return Ok(VerificationQuery {
                value: Self::new(self.kind(), self.capacity().max(other.capacity())),
                evidence: environment.query_evidence(start, node_limit, remaining),
            });
        }
        let value = match (self.normalized_slots(), other.normalized_slots()) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(left), Some(right)) => {
                let mut slots = Vec::with_capacity(left.len());
                for (a, b) in left.iter().zip(right) {
                    slots.push(match (a, b) {
                        (Slot::Value(a), Slot::Value(b)) => Slot::Value(
                            environment
                                .join_types_inner(a, b, node_limit, &mut remaining)
                                .map_err(|error| {
                                    environment.query_failure(error, start, node_limit, remaining)
                                })?
                                .value,
                        ),
                        (Slot::Category2Tail, Slot::Category2Tail) => Slot::Category2Tail,
                        (Slot::Unusable, Slot::Unusable) => Slot::Unusable,
                        _ => Slot::Unusable,
                    });
                }
                let mut frame = Self::Reachable {
                    kind: self.kind(),
                    slots: slots.into_boxed_slice(),
                };
                normalize_category2(&mut frame);
                frame
            }
        };
        Ok(VerificationQuery {
            value,
            evidence: environment.query_evidence(start, node_limit, remaining),
        })
    }

    /// Returns the frame kind.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::Bottom { kind, .. } | Self::Reachable { kind, .. } => *kind,
        }
    }

    /// Returns the fixed slot capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        match self {
            Self::Bottom { capacity, .. } => *capacity,
            Self::Reachable { slots, .. } => slots.len(),
        }
    }

    /// Reads the value beginning at `index`; tails and unusable slots read as `None`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&VerificationType> {
        match self {
            Self::Reachable { slots, .. } => match slots.get(index) {
                Some(Slot::Value(value)) => Some(value),
                _ => None,
            },
            Self::Bottom { .. } => None,
        }
    }

    /// Writes a local while preserving the invariant that category-2 halves never survive.
    pub fn set_local(&mut self, index: usize, value: VerificationType) -> Result<(), FrameError> {
        let Self::Reachable { kind, slots } = self else {
            return Err(FrameError::OutOfBounds);
        };
        if *kind != FrameKind::Locals {
            return Err(FrameError::WrongKind);
        }
        let width = value.width().ok_or(FrameError::OutOfBounds)?;
        if index >= slots.len() {
            return Err(FrameError::OutOfBounds);
        }
        if width == VerificationTypeWidth::Category2 && index + 1 >= slots.len() {
            return Err(FrameError::TruncatedCategory2);
        }
        let overwrote_tail = matches!(slots[index], Slot::Category2Tail);
        invalidate_value_at(slots, index);
        if index > 0 && overwrote_tail {
            invalidate_value_at(slots, index - 1);
        }
        slots[index] = Slot::Value(value);
        if width == VerificationTypeWidth::Category2 {
            invalidate_value_at(slots, index + 1);
            slots[index + 1] = Slot::Category2Tail;
        }
        Ok(())
    }

    /// Pushes one value onto an operand frame, charging its JVM category width.
    pub fn push(&mut self, value: VerificationType) -> Result<(), FrameError> {
        let Self::Reachable { kind, slots } = self else {
            return Err(FrameError::OutOfBounds);
        };
        if *kind != FrameKind::OperandStack {
            return Err(FrameError::WrongKind);
        }
        let width = match value.width() {
            Some(VerificationTypeWidth::Category1) => 1,
            Some(VerificationTypeWidth::Category2) => 2,
            None => return Err(FrameError::OutOfBounds),
        };
        let start = slots
            .iter()
            .position(|slot| matches!(slot, Slot::Unusable))
            .unwrap_or(slots.len());
        if start + width > slots.len() {
            return Err(FrameError::TruncatedCategory2);
        }
        slots[start] = Slot::Value(value);
        if width == 2 {
            slots[start + 1] = Slot::Category2Tail;
        }
        Ok(())
    }

    fn normalized_slots(&self) -> Option<&[Slot]> {
        match self {
            Self::Reachable { slots, .. } => Some(slots),
            Self::Bottom { .. } => None,
        }
    }
}

/// Applies one constants, locals, or stack transfer rule atomically.
pub fn transfer_storage_instruction<R: VerificationConstantResolver>(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    constants: &R,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let operands = instruction.instruction().operands.as_slice();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let mut next = state.clone();
    if opcode == Opcode::Nop {
        return operands
            .is_empty()
            .then_some(next)
            .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let constant = match opcode {
        Opcode::AconstNull => Some(VerificationType::Null),
        Opcode::IconstM1
        | Opcode::Iconst0
        | Opcode::Iconst1
        | Opcode::Iconst2
        | Opcode::Iconst3
        | Opcode::Iconst4
        | Opcode::Iconst5
        | Opcode::Bipush
        | Opcode::Sipush => Some(VerificationType::Int),
        Opcode::Lconst0 | Opcode::Lconst1 => Some(VerificationType::Long),
        Opcode::Fconst0 | Opcode::Fconst1 | Opcode::Fconst2 => Some(VerificationType::Float),
        Opcode::Dconst0 | Opcode::Dconst1 => Some(VerificationType::Double),
        Opcode::Ldc | Opcode::LdcW | Opcode::Ldc2W => {
            let [InstructionOperand::Constant(index)] = operands else {
                return Err(fail(VerificationTransferKind::MalformedPreparedInput));
            };
            let value = constants
                .verification_type(*index)
                .ok_or_else(|| fail(VerificationTransferKind::Constant { index: *index }))?;
            let wanted = if opcode == Opcode::Ldc2W { 2 } else { 1 };
            if type_width(&value) != wanted {
                return Err(fail(VerificationTransferKind::Constant { index: *index }));
            }
            Some(value)
        }
        _ => None,
    };
    if let Some(value) = constant {
        if !constant_operands_valid(opcode, operands) {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        }
        next.stack
            .push(value)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if let Some((slot, expected)) =
        verifier_local_access(opcode, operands, true).map_err(&fail)?
    {
        let value = next
            .locals
            .get(slot)
            .cloned()
            .ok_or_else(|| fail(VerificationTransferKind::LocalBounds))?;
        require_verification_category(&value, expected)
            .then_some(())
            .ok_or_else(|| fail(VerificationTransferKind::Category))?;
        next.stack
            .push(value)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if let Some((slot, expected)) =
        verifier_local_access(opcode, operands, false).map_err(&fail)?
    {
        let mut values = stack_values(&next.stack);
        let value = values
            .pop()
            .ok_or_else(|| fail(VerificationTransferKind::StackBounds))?;
        require_verification_category(&value, expected)
            .then_some(())
            .ok_or_else(|| fail(VerificationTransferKind::Category))?;
        next.locals
            .set_local(slot, value)
            .map_err(|_| fail(VerificationTransferKind::LocalBounds))?;
        next.stack = stack_from_values(next.stack.capacity(), values)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if opcode == Opcode::Iinc {
        let [
            InstructionOperand::Local(slot),
            InstructionOperand::Immediate(_),
        ] = operands
        else {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        };
        let slot = usize::from(*slot);
        let value = next
            .locals
            .get(slot)
            .ok_or_else(|| fail(VerificationTransferKind::LocalBounds))?;
        if value != &VerificationType::Int {
            return Err(fail(VerificationTransferKind::Category));
        }
        return Ok(next);
    }
    if let Some(choices) = crate::execution::shuffle_descriptor(opcode) {
        if !operands.is_empty() {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        }
        let values = stack_values(&next.stack);
        let widths: Vec<_> = values.iter().map(type_width).collect();
        let Some((input, output)) = choices.iter().find(|(input, _)| widths.ends_with(input))
        else {
            return Err(fail(
                if widths.iter().sum::<usize>() < instruction.input_width() {
                    VerificationTransferKind::StackBounds
                } else {
                    VerificationTransferKind::Category
                },
            ));
        };
        let prefix = values.len() - input.len();
        let mut shuffled = values[..prefix].to_vec();
        shuffled.extend(output.iter().map(|index| values[prefix + index].clone()));
        next.stack = stack_from_values(next.stack.capacity(), shuffled)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    Err(fail(VerificationTransferKind::MalformedPreparedInput))
}

#[derive(Clone, Copy)]
enum VerificationCategory {
    Int,
    Long,
    Float,
    Double,
    Reference,
}

fn type_width(value: &VerificationType) -> usize {
    usize::from(value.width() == Some(VerificationTypeWidth::Category2)) + 1
}

fn stack_values(frame: &VerificationFrame) -> Vec<VerificationType> {
    frame
        .normalized_slots()
        .into_iter()
        .flatten()
        .filter_map(|slot| match slot {
            Slot::Value(value) => Some(value.clone()),
            Slot::Unusable | Slot::Category2Tail => None,
        })
        .collect()
}

fn stack_from_values(
    capacity: usize,
    values: Vec<VerificationType>,
) -> Result<VerificationFrame, FrameError> {
    let mut frame = VerificationFrame::new(FrameKind::OperandStack, capacity);
    for value in values {
        frame.push(value)?;
    }
    Ok(frame)
}

fn constant_operands_valid(opcode: Opcode, operands: &[InstructionOperand]) -> bool {
    match opcode {
        Opcode::Bipush | Opcode::Sipush => matches!(operands, [InstructionOperand::Immediate(_)]),
        Opcode::Ldc | Opcode::LdcW | Opcode::Ldc2W => {
            matches!(operands, [InstructionOperand::Constant(_)])
        }
        _ => operands.is_empty(),
    }
}

fn require_verification_category(value: &VerificationType, expected: VerificationCategory) -> bool {
    matches!(
        (expected, value),
        (VerificationCategory::Int, VerificationType::Int)
            | (VerificationCategory::Long, VerificationType::Long)
            | (VerificationCategory::Float, VerificationType::Float)
            | (VerificationCategory::Double, VerificationType::Double)
            | (
                VerificationCategory::Reference,
                VerificationType::Null
                    | VerificationType::Reference(_)
                    | VerificationType::UninitializedThis
                    | VerificationType::Uninitialized(_)
            )
    )
}

fn verifier_local_access(
    opcode: Opcode,
    operands: &[InstructionOperand],
    load: bool,
) -> Result<Option<(usize, VerificationCategory)>, VerificationTransferKind> {
    use Opcode::*;
    let (category, fixed) = match (load, opcode) {
        (true, Iload) | (false, Istore) => (VerificationCategory::Int, None),
        (true, Lload) | (false, Lstore) => (VerificationCategory::Long, None),
        (true, Fload) | (false, Fstore) => (VerificationCategory::Float, None),
        (true, Dload) | (false, Dstore) => (VerificationCategory::Double, None),
        (true, Aload) | (false, Astore) => (VerificationCategory::Reference, None),
        _ => match verifier_implicit_local(opcode, load) {
            Some(value) => value,
            None => return Ok(None),
        },
    };
    let slot = match fixed {
        Some(slot) if operands.is_empty() => slot,
        Some(_) => return Err(VerificationTransferKind::MalformedPreparedInput),
        None => match operands {
            [InstructionOperand::Local(slot)] => usize::from(*slot),
            _ => return Err(VerificationTransferKind::MalformedPreparedInput),
        },
    };
    Ok(Some((slot, category)))
}

fn verifier_implicit_local(
    opcode: Opcode,
    load: bool,
) -> Option<(VerificationCategory, Option<usize>)> {
    use Opcode::*;
    let families = if load {
        [
            (Iload0, VerificationCategory::Int),
            (Lload0, VerificationCategory::Long),
            (Fload0, VerificationCategory::Float),
            (Dload0, VerificationCategory::Double),
            (Aload0, VerificationCategory::Reference),
        ]
    } else {
        [
            (Istore0, VerificationCategory::Int),
            (Lstore0, VerificationCategory::Long),
            (Fstore0, VerificationCategory::Float),
            (Dstore0, VerificationCategory::Double),
            (Astore0, VerificationCategory::Reference),
        ]
    };
    families.into_iter().find_map(|(first, category)| {
        let opcode = opcode as u8 as usize;
        let first = first as u8 as usize;
        (opcode >= first && opcode - first < 4).then_some((category, Some(opcode - first)))
    })
}

fn invalidate_value_at(slots: &mut [Slot], index: usize) {
    if matches!(slots.get(index), Some(Slot::Value(value)) if value.width() == Some(VerificationTypeWidth::Category2))
        && index + 1 < slots.len()
    {
        slots[index + 1] = Slot::Unusable;
    }
    slots[index] = Slot::Unusable;
}
