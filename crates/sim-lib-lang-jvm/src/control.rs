//! Exact, resumable execution of JVM control-flow instructions.

use sim_codec_classfile::{InstructionId, Opcode};
use sim_lib_machine::{CodeCursor, LocatedCode, SourceLocation, StackError, UnitStack};

use crate::{JvmReference, JvmValue, JvmValueWidth, PreparedJvmPolicy};

/// The successful disposition of one control-flow instruction.
#[derive(Clone, Debug)]
pub enum JvmControlOutcome {
    /// Continue at an instruction-boundary cursor minted by the prepared code.
    Continue {
        /// Exact next instruction.
        cursor: CodeCursor,
        /// Exact work charged for the completed instruction.
        receipt: crate::JvmWorkReceipt,
    },
    /// Complete the current method, optionally transferring its return value.
    Return {
        /// `None` for `return`; otherwise the primitive or reference result.
        value: Option<JvmValue>,
        /// Exact work charged for the completed instruction.
        receipt: crate::JvmWorkReceipt,
    },
    /// Stop before a safepoint instruction. Resumption uses this same cursor and charges no work.
    Interrupted {
        /// Stable instruction identity at which execution stopped.
        instruction: InstructionId,
        /// Exact cursor to supply when resuming.
        cursor: CodeCursor,
    },
}

/// Located refusal from control-flow execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JvmControlError {
    /// Stable identity of the refused instruction.
    pub instruction: InstructionId,
    /// Immutable source location of the refused instruction.
    pub location: Box<SourceLocation>,
    /// Exact refusal category.
    pub kind: JvmControlErrorKind,
}

/// Exact control-flow refusal category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JvmControlErrorKind {
    /// The prepared operand shape or target count is inconsistent with the opcode.
    MalformedPreparedInput,
    /// The live operand stack refused an operation.
    Stack(StackError),
    /// A live value has the wrong JVM computational category.
    Category,
    /// A non-returning instruction at code end has no valid fallthrough cursor.
    MissingFallthrough,
}

/// Executes one prepared branch, switch, or return.
///
/// When `interrupt_requested` is true at a declared safepoint, execution stops before touching
/// operands. The returned cursor identifies the same instruction, making retry exact.
pub fn execute_control_instruction(
    code: &LocatedCode<PreparedJvmPolicy>,
    cursor: CodeCursor,
    operands: &mut UnitStack<JvmValueWidth>,
    interrupt_requested: bool,
) -> Result<JvmControlOutcome, JvmControlError> {
    let located = code.instruction(cursor);
    let instruction = located.instruction();
    let id = instruction.id();
    if interrupt_requested && located.is_safepoint() {
        return Ok(JvmControlOutcome::Interrupted {
            instruction: id,
            cursor,
        });
    }
    let opcode = instruction.opcode();
    let targets = code.branch_targets(id);
    let malformed = || error(located, JvmControlErrorKind::MalformedPreparedInput);
    let fallthrough = || {
        code.next(cursor)
            .ok_or_else(|| error(located, JvmControlErrorKind::MissingFallthrough))
    };
    let complete = |cursor| JvmControlOutcome::Continue {
        cursor,
        receipt: crate::JvmWorkReceipt::new(id, instruction.work_charge()),
    };

    use Opcode::*;
    let selected = match opcode {
        Goto | GotoW => Some(only_target(targets).ok_or_else(malformed)?),
        Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle => {
            let target = direct_target(code, instruction).ok_or_else(malformed)?;
            let next = fallthrough()?;
            let value = peek_int(operands, located)?;
            let take = match opcode {
                Ifeq => value == 0,
                Ifne => value != 0,
                Iflt => value < 0,
                Ifge => value >= 0,
                Ifgt => value > 0,
                Ifle => value <= 0,
                _ => unreachable!(),
            };
            operands.pop().expect("validated conditional operand");
            Some(if take { target } else { next })
        }
        IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge | IfIcmpgt | IfIcmple => {
            let target = direct_target(code, instruction).ok_or_else(malformed)?;
            let next = fallthrough()?;
            let (left, right) = two_ints(operands, located)?;
            operands.pop().expect("validated top operand");
            operands.pop().expect("validated lower operand");
            let take = match opcode {
                IfIcmpeq => left == right,
                IfIcmpne => left != right,
                IfIcmplt => left < right,
                IfIcmpge => left >= right,
                IfIcmpgt => left > right,
                IfIcmple => left <= right,
                _ => unreachable!(),
            };
            Some(if take { target } else { next })
        }
        IfAcmpeq | IfAcmpne => {
            let target = direct_target(code, instruction).ok_or_else(malformed)?;
            let next = fallthrough()?;
            let (left, right) = two_references(operands, located)?;
            operands.pop().expect("validated top operand");
            operands.pop().expect("validated lower operand");
            Some(if (opcode == IfAcmpeq) == (left == right) {
                target
            } else {
                next
            })
        }
        Ifnull | Ifnonnull => {
            let target = direct_target(code, instruction).ok_or_else(malformed)?;
            let next = fallthrough()?;
            let value = peek_reference(operands, located)?;
            operands.pop().expect("validated conditional operand");
            Some(if (opcode == Ifnull) == (value == JvmReference::NULL) {
                target
            } else {
                next
            })
        }
        Tableswitch => {
            let key = peek_int(operands, located)?;
            let target =
                table_target(code, instruction.prepared_operands(), key).ok_or_else(malformed)?;
            operands.pop().expect("validated switch operand");
            Some(target)
        }
        Lookupswitch => {
            let key = peek_int(operands, located)?;
            let target =
                lookup_target(code, instruction.prepared_operands(), key).ok_or_else(malformed)?;
            operands.pop().expect("validated switch operand");
            Some(target)
        }
        Ireturn | Lreturn | Freturn | Dreturn | Areturn => {
            if !matches!(
                instruction.prepared_operands(),
                crate::PreparedJvmOperands::None
            ) || !targets.is_empty()
            {
                return Err(malformed());
            }
            let value = operands
                .top()
                .map_err(|e| error(located, JvmControlErrorKind::Stack(e)))?;
            let valid = matches!(
                (opcode, value),
                (Ireturn, JvmValue::Int(_))
                    | (Lreturn, JvmValue::Long(_))
                    | (Freturn, JvmValue::Float(_))
                    | (Dreturn, JvmValue::Double(_))
                    | (Areturn, JvmValue::Reference(_))
            );
            if !valid {
                return Err(error(located, JvmControlErrorKind::Category));
            }
            let value = operands.pop().expect("validated return operand");
            return Ok(JvmControlOutcome::Return {
                value: Some(value),
                receipt: crate::JvmWorkReceipt::new(id, instruction.work_charge()),
            });
        }
        Return => {
            if !matches!(
                instruction.prepared_operands(),
                crate::PreparedJvmOperands::None
            ) || !targets.is_empty()
            {
                return Err(malformed());
            }
            return Ok(JvmControlOutcome::Return {
                value: None,
                receipt: crate::JvmWorkReceipt::new(id, instruction.work_charge()),
            });
        }
        _ => return Err(malformed()),
    };
    Ok(complete(match selected {
        Some(target) => target,
        None => fallthrough()?,
    }))
}

fn error(
    located: &sim_lib_machine::LocatedInstruction<crate::PreparedJvmInstruction, InstructionId>,
    kind: JvmControlErrorKind,
) -> JvmControlError {
    JvmControlError {
        instruction: *located.id(),
        location: Box::new(located.location().clone()),
        kind,
    }
}

fn only_target(targets: &[CodeCursor]) -> Option<CodeCursor> {
    let [target] = targets else { return None };
    Some(*target)
}

fn direct_target(
    code: &LocatedCode<PreparedJvmPolicy>,
    instruction: &crate::PreparedJvmInstruction,
) -> Option<CodeCursor> {
    let crate::PreparedJvmOperands::Direct(target) = instruction.prepared_operands() else {
        return None;
    };
    code.cursor(*target)
}

fn peek_int(
    stack: &UnitStack<JvmValueWidth>,
    located: &sim_lib_machine::LocatedInstruction<crate::PreparedJvmInstruction, InstructionId>,
) -> Result<i32, JvmControlError> {
    match stack
        .top()
        .map_err(|e| error(located, JvmControlErrorKind::Stack(e)))?
    {
        JvmValue::Int(value) => Ok(*value),
        _ => Err(error(located, JvmControlErrorKind::Category)),
    }
}

fn peek_reference(
    stack: &UnitStack<JvmValueWidth>,
    located: &sim_lib_machine::LocatedInstruction<crate::PreparedJvmInstruction, InstructionId>,
) -> Result<JvmReference, JvmControlError> {
    match stack
        .top()
        .map_err(|e| error(located, JvmControlErrorKind::Stack(e)))?
    {
        JvmValue::Reference(value) => Ok(*value),
        _ => Err(error(located, JvmControlErrorKind::Category)),
    }
}

fn two_ints(
    stack: &UnitStack<JvmValueWidth>,
    located: &sim_lib_machine::LocatedInstruction<crate::PreparedJvmInstruction, InstructionId>,
) -> Result<(i32, i32), JvmControlError> {
    let mut values = Vec::new();
    stack.visit_values(|value| values.push(value.clone()));
    if values.len() < 2 {
        return Err(error(
            located,
            JvmControlErrorKind::Stack(StackError::Underflow {
                depth: stack.depth(),
            }),
        ));
    }
    match values[values.len() - 2..] {
        [JvmValue::Int(left), JvmValue::Int(right)] => Ok((left, right)),
        _ => Err(error(located, JvmControlErrorKind::Category)),
    }
}

fn two_references(
    stack: &UnitStack<JvmValueWidth>,
    located: &sim_lib_machine::LocatedInstruction<crate::PreparedJvmInstruction, InstructionId>,
) -> Result<(JvmReference, JvmReference), JvmControlError> {
    let mut values = Vec::new();
    stack.visit_values(|value| values.push(value.clone()));
    if values.len() < 2 {
        return Err(error(
            located,
            JvmControlErrorKind::Stack(StackError::Underflow {
                depth: stack.depth(),
            }),
        ));
    }
    match values[values.len() - 2..] {
        [JvmValue::Reference(left), JvmValue::Reference(right)] => Ok((left, right)),
        _ => Err(error(located, JvmControlErrorKind::Category)),
    }
}

fn table_target(
    code: &LocatedCode<PreparedJvmPolicy>,
    operands: &crate::PreparedJvmOperands,
    key: i32,
) -> Option<CodeCursor> {
    let crate::PreparedJvmOperands::Table {
        low,
        default,
        targets,
    } = operands
    else {
        return None;
    };
    let count = i64::try_from(targets.len()).ok()?;
    let index = i64::from(key) - i64::from(*low);
    code.cursor(if index >= 0 && index < count {
        targets[usize::try_from(index).ok()?]
    } else {
        *default
    })
}

fn lookup_target(
    code: &LocatedCode<PreparedJvmPolicy>,
    operands: &crate::PreparedJvmOperands,
    key: i32,
) -> Option<CodeCursor> {
    let crate::PreparedJvmOperands::Lookup { default, pairs } = operands else {
        return None;
    };
    let target = pairs
        .binary_search_by_key(&key, |(candidate, _)| *candidate)
        .map_or(*default, |index| pairs[index].1);
    code.cursor(target)
}
