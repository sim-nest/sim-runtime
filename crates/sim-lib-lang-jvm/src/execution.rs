//! Execution of the JVM constant, local, and operand-stack instruction family.

use sim_codec_classfile::{InstructionId, InstructionOperand, Opcode};
use sim_lib_machine::{ShuffleError, ShufflePlan, SlotError, SlotFile, StackError, UnitStack};

use crate::{JvmReference, JvmValue, JvmValueWidth};

/// Resolves a prepared constant-pool reference to its runtime value.
pub trait JvmConstantResolver {
    /// Returns the constant denoted by `index`, or refuses the prepared reference.
    fn resolve(&mut self, index: u16) -> Result<JvmValue, ExecutionError>;
}

impl<F> JvmConstantResolver for F
where
    F: FnMut(u16) -> Result<JvmValue, ExecutionError>,
{
    fn resolve(&mut self, index: u16) -> Result<JvmValue, ExecutionError> {
        self(index)
    }
}

/// Exact evidence for one successfully executed prepared instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JvmWorkReceipt {
    instruction: InstructionId,
    charged: usize,
}

impl JvmWorkReceipt {
    pub(crate) const fn new(instruction: InstructionId) -> Self {
        Self {
            instruction,
            charged: 1,
        }
    }
    /// Returns the prepared instruction charged by this receipt.
    pub const fn instruction(self) -> InstructionId {
        self.instruction
    }

    /// Returns the exact instruction-work charge.
    pub const fn charged(self) -> usize {
        self.charged
    }
}

/// Typed refusal from this JVM execution family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionError {
    /// The prepared opcode or operand shape is outside this execution family.
    MalformedPreparedInput {
        /// Opcode whose prepared record or live category layout was invalid.
        opcode: Opcode,
    },
    /// A local slot operation was refused by the shared machine.
    Local(SlotError),
    /// An operand-stack operation was refused by the shared machine.
    Stack(StackError),
    /// A validated whole-value shuffle was refused by the shared machine.
    Shuffle(ShuffleError),
    /// A load or store encountered a value of the wrong JVM computational category.
    Category {
        /// Opcode whose required computational category was not present.
        opcode: Opcode,
    },
    /// A constant-pool resolver refused an entry.
    Constant {
        /// Refused constant-pool index.
        index: u16,
    },
}

/// Executes one prepared constant, local, or stack instruction.
///
/// Every operand permutation is expressed as a shared-machine [`ShufflePlan`]. A receipt is
/// returned only after the complete operation succeeds, so refused input never charges work.
pub fn execute_storage_instruction<R: JvmConstantResolver>(
    instruction: &crate::PreparedJvmInstruction,
    locals: &mut SlotFile<JvmValueWidth>,
    operands: &mut UnitStack<JvmValueWidth>,
    constants: &mut R,
) -> Result<JvmWorkReceipt, ExecutionError> {
    let opcode = instruction.opcode();
    let decoded = instruction.instruction();
    let value = match opcode {
        Opcode::AconstNull => Some(JvmValue::Reference(JvmReference::NULL)),
        Opcode::IconstM1 => Some(JvmValue::Int(-1)),
        Opcode::Iconst0
        | Opcode::Iconst1
        | Opcode::Iconst2
        | Opcode::Iconst3
        | Opcode::Iconst4
        | Opcode::Iconst5 => Some(JvmValue::Int(opcode as i32 - Opcode::Iconst0 as i32)),
        Opcode::Lconst0 | Opcode::Lconst1 => Some(JvmValue::Long(
            (opcode as u8 - Opcode::Lconst0 as u8).into(),
        )),
        Opcode::Fconst0 | Opcode::Fconst1 | Opcode::Fconst2 => Some(JvmValue::Float(
            f32::from(opcode as u8 - Opcode::Fconst0 as u8).to_bits(),
        )),
        Opcode::Dconst0 | Opcode::Dconst1 => Some(JvmValue::Double(
            f64::from(opcode as u8 - Opcode::Dconst0 as u8).to_bits(),
        )),
        Opcode::Bipush | Opcode::Sipush => Some(JvmValue::Int(one_immediate(
            decoded.operands.as_slice(),
            opcode,
        )?)),
        Opcode::Ldc | Opcode::LdcW | Opcode::Ldc2W => {
            let index = one_constant(decoded.operands.as_slice(), opcode)?;
            let value = constants
                .resolve(index)
                .map_err(|_| ExecutionError::Constant { index })?;
            let expected_width = usize::from(opcode == Opcode::Ldc2W) + 1;
            if value.logical_width() != expected_width {
                return Err(ExecutionError::Category { opcode });
            }
            Some(value)
        }
        _ => None,
    };
    if let Some(value) = value {
        operands.push(value).map_err(ExecutionError::Stack)?;
        return Ok(receipt(instruction));
    }

    if let Some((slot, expected)) = local_access(opcode, decoded.operands.as_slice(), true)? {
        let value = locals.load(slot).map_err(ExecutionError::Local)?;
        require_category(opcode, value, expected)?;
        operands
            .push(value.clone())
            .map_err(ExecutionError::Stack)?;
        return Ok(receipt(instruction));
    }
    if let Some((slot, expected)) = local_access(opcode, decoded.operands.as_slice(), false)? {
        let value = operands.top().map_err(ExecutionError::Stack)?;
        require_category(opcode, value, expected)?;
        if slot
            .checked_add(value.logical_width())
            .is_none_or(|end| end > locals.limit())
        {
            return Err(ExecutionError::Local(SlotError::Overflow {
                slot,
                width: value.logical_width(),
                limit: locals.limit(),
            }));
        }
        let value = operands.pop().map_err(ExecutionError::Stack)?;
        locals.store(slot, value).map_err(ExecutionError::Local)?;
        return Ok(receipt(instruction));
    }
    if opcode == Opcode::Iinc {
        let [
            InstructionOperand::Local(slot),
            InstructionOperand::Immediate(increment),
        ] = decoded.operands.as_slice()
        else {
            return Err(ExecutionError::MalformedPreparedInput { opcode });
        };
        let JvmValue::Int(value) = locals
            .load(usize::from(*slot))
            .map_err(ExecutionError::Local)?
        else {
            return Err(ExecutionError::Category { opcode });
        };
        let value = value.wrapping_add(*increment);
        locals
            .store(usize::from(*slot), JvmValue::Int(value))
            .map_err(ExecutionError::Local)?;
        return Ok(receipt(instruction));
    }
    if matches!(
        opcode,
        Opcode::Pop
            | Opcode::Pop2
            | Opcode::Dup
            | Opcode::DupX1
            | Opcode::DupX2
            | Opcode::Dup2
            | Opcode::Dup2X1
            | Opcode::Dup2X2
            | Opcode::Swap
    ) {
        if !decoded.operands.is_empty() {
            return Err(ExecutionError::MalformedPreparedInput { opcode });
        }
        execute_shuffle(opcode, operands)?;
        return Ok(receipt(instruction));
    }
    Err(ExecutionError::MalformedPreparedInput { opcode })
}

#[derive(Clone, Copy)]
enum Expected {
    Int,
    Long,
    Float,
    Double,
    Reference,
}

fn require_category(
    opcode: Opcode,
    value: &JvmValue,
    expected: Expected,
) -> Result<(), ExecutionError> {
    let matches = matches!(
        (expected, value),
        (Expected::Int, JvmValue::Int(_))
            | (Expected::Long, JvmValue::Long(_))
            | (Expected::Float, JvmValue::Float(_))
            | (Expected::Double, JvmValue::Double(_))
            | (Expected::Reference, JvmValue::Reference(_))
    );
    matches
        .then_some(())
        .ok_or(ExecutionError::Category { opcode })
}

fn local_access(
    opcode: Opcode,
    operands: &[InstructionOperand],
    load: bool,
) -> Result<Option<(usize, Expected)>, ExecutionError> {
    use Opcode::*;
    let (base, fixed) = match (load, opcode) {
        (true, Iload) => (Expected::Int, None),
        (true, Lload) => (Expected::Long, None),
        (true, Fload) => (Expected::Float, None),
        (true, Dload) => (Expected::Double, None),
        (true, Aload) => (Expected::Reference, None),
        (false, Istore) => (Expected::Int, None),
        (false, Lstore) => (Expected::Long, None),
        (false, Fstore) => (Expected::Float, None),
        (false, Dstore) => (Expected::Double, None),
        (false, Astore) => (Expected::Reference, None),
        _ => match implicit_local(opcode, load) {
            Some(access) => access,
            None => return Ok(None),
        },
    };
    let slot = if let Some(slot) = fixed {
        if !operands.is_empty() {
            return Err(ExecutionError::MalformedPreparedInput { opcode });
        }
        slot
    } else {
        let [InstructionOperand::Local(slot)] = operands else {
            return Err(ExecutionError::MalformedPreparedInput { opcode });
        };
        usize::from(*slot)
    };
    Ok(Some((slot, base)))
}

fn implicit_local(opcode: Opcode, load: bool) -> Option<(Expected, Option<usize>)> {
    use Opcode::*;
    let families = if load {
        [
            (Iload0, Expected::Int),
            (Lload0, Expected::Long),
            (Fload0, Expected::Float),
            (Dload0, Expected::Double),
            (Aload0, Expected::Reference),
        ]
    } else {
        [
            (Istore0, Expected::Int),
            (Lstore0, Expected::Long),
            (Fstore0, Expected::Float),
            (Dstore0, Expected::Double),
            (Astore0, Expected::Reference),
        ]
    };
    families.into_iter().find_map(|(first, expected)| {
        let offset = opcode as u8 as usize - (first as u8 as usize).min(opcode as u8 as usize);
        (offset < 4 && opcode as u8 as usize >= first as u8 as usize)
            .then_some((expected, Some(offset)))
    })
}

fn execute_shuffle(
    opcode: Opcode,
    stack: &mut UnitStack<JvmValueWidth>,
) -> Result<(), ExecutionError> {
    let mut widths = Vec::new();
    stack.visit_values(|value| widths.push(value.logical_width()));
    let choices =
        shuffle_descriptor(opcode).ok_or(ExecutionError::MalformedPreparedInput { opcode })?;
    let Some((input, output)) = choices.iter().find(|(input, _)| widths.ends_with(input)) else {
        return Err(ExecutionError::MalformedPreparedInput { opcode });
    };
    let prefix = widths.len() - input.len();
    let mut groups: Vec<usize> = (0..prefix).collect();
    groups.extend(output.iter().map(|group| prefix + group));
    let starts: Vec<usize> = widths
        .iter()
        .scan(0, |start, width| {
            let current = *start;
            *start += width;
            Some(current)
        })
        .collect();
    let units: Vec<_> = groups
        .into_iter()
        .flat_map(|group| starts[group]..starts[group] + widths[group])
        .collect();
    ShufflePlan::new(widths, units)
        .map_err(ExecutionError::Shuffle)?
        .execute(stack)
        .map_err(ExecutionError::Shuffle)
}

/// Whole-value JVM forms shared by execution and verification.
pub(crate) fn shuffle_descriptor(
    opcode: Opcode,
) -> Option<&'static [(&'static [usize], &'static [usize])]> {
    Some(match opcode {
        Opcode::Pop => &[(&[1], &[])],
        Opcode::Pop2 => &[(&[2], &[]), (&[1, 1], &[])],
        Opcode::Dup => &[(&[1], &[0, 0])],
        Opcode::DupX1 => &[(&[1, 1], &[1, 0, 1])],
        Opcode::DupX2 => &[(&[2, 1], &[1, 0, 1]), (&[1, 1, 1], &[2, 0, 1, 2])],
        Opcode::Dup2 => &[(&[2], &[0, 0]), (&[1, 1], &[0, 1, 0, 1])],
        Opcode::Dup2X1 => &[(&[1, 2], &[1, 0, 1]), (&[1, 1, 1], &[1, 2, 0, 1, 2])],
        Opcode::Dup2X2 => &[
            (&[2, 2], &[1, 0, 1]),
            (&[1, 1, 2], &[2, 0, 1, 2]),
            (&[2, 1, 1], &[1, 2, 0, 1, 2]),
            (&[1, 1, 1, 1], &[2, 3, 0, 1, 2, 3]),
        ],
        Opcode::Swap => &[(&[1, 1], &[1, 0])],
        _ => return None,
    })
}

fn one_immediate(operands: &[InstructionOperand], opcode: Opcode) -> Result<i32, ExecutionError> {
    let [InstructionOperand::Immediate(value)] = operands else {
        return Err(ExecutionError::MalformedPreparedInput { opcode });
    };
    Ok(*value)
}
fn one_constant(operands: &[InstructionOperand], opcode: Opcode) -> Result<u16, ExecutionError> {
    let [InstructionOperand::Constant(value)] = operands else {
        return Err(ExecutionError::MalformedPreparedInput { opcode });
    };
    Ok(*value)
}
fn receipt(instruction: &crate::PreparedJvmInstruction) -> JvmWorkReceipt {
    JvmWorkReceipt::new(instruction.id())
}
