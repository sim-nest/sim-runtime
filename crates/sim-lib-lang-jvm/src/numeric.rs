//! Exact JVM numeric instruction policy.

use sim_codec_classfile::Opcode;
use sim_lib_machine::{StackError, UnitStack};

use crate::{JavaThrowable, JvmValue, JvmValueWidth, JvmWorkReceipt};

/// Failure while executing one numeric instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumericExecutionError {
    /// The opcode is not numeric or the live operand categories do not match it.
    Category {
        /// Opcode whose required categories were not present.
        opcode: Opcode,
    },
    /// Shared bounded operand storage refused the operation.
    Stack(StackError),
    /// Java integer division or remainder by zero raised `ArithmeticException`.
    Arithmetic(Box<JavaThrowable>),
}

/// Executes one prepared JVM arithmetic, bitwise, shift, comparison, or conversion instruction.
///
/// `arithmetic_throwable` allocates the guest object and landed [`sim_lib_control::Raised`]
/// envelope only on the exceptional integer divide/remainder path.
pub fn execute_numeric_instruction<F>(
    instruction: &crate::PreparedJvmInstruction,
    operands: &mut UnitStack<JvmValueWidth>,
    mut arithmetic_throwable: F,
) -> Result<JvmWorkReceipt, NumericExecutionError>
where
    F: FnMut() -> JavaThrowable,
{
    let opcode = instruction.opcode();
    let right = operands.pop().map_err(NumericExecutionError::Stack)?;
    let result = match opcode {
        Opcode::Ineg => JvmValue::Int(expect_i(opcode, right)?.wrapping_neg()),
        Opcode::Lneg => JvmValue::Long(expect_l(opcode, right)?.wrapping_neg()),
        Opcode::Fneg => JvmValue::Float(expect_f(opcode, right)? ^ (1 << 31)),
        Opcode::Dneg => JvmValue::Double(expect_d(opcode, right)? ^ (1 << 63)),
        Opcode::I2l => JvmValue::Long(i64::from(expect_i(opcode, right)?)),
        Opcode::I2f => JvmValue::Float((expect_i(opcode, right)? as f32).to_bits()),
        Opcode::I2d => JvmValue::Double((expect_i(opcode, right)? as f64).to_bits()),
        Opcode::L2i => JvmValue::Int(expect_l(opcode, right)? as i32),
        Opcode::L2f => JvmValue::Float((expect_l(opcode, right)? as f32).to_bits()),
        Opcode::L2d => JvmValue::Double((expect_l(opcode, right)? as f64).to_bits()),
        Opcode::F2i => JvmValue::Int(float_to_i32(f64::from(f32::from_bits(expect_f(
            opcode, right,
        )?)))),
        Opcode::F2l => JvmValue::Long(float_to_i64(f64::from(f32::from_bits(expect_f(
            opcode, right,
        )?)))),
        Opcode::F2d => {
            JvmValue::Double(f64::from(f32::from_bits(expect_f(opcode, right)?)).to_bits())
        }
        Opcode::D2i => JvmValue::Int(float_to_i32(f64::from_bits(expect_d(opcode, right)?))),
        Opcode::D2l => JvmValue::Long(float_to_i64(f64::from_bits(expect_d(opcode, right)?))),
        Opcode::D2f => JvmValue::Float((f64::from_bits(expect_d(opcode, right)?) as f32).to_bits()),
        Opcode::I2b => JvmValue::Int(i32::from(expect_i(opcode, right)? as i8)),
        Opcode::I2c => JvmValue::Int(i32::from(expect_i(opcode, right)? as u16)),
        Opcode::I2s => JvmValue::Int(i32::from(expect_i(opcode, right)? as i16)),
        _ => {
            let left = operands.pop().map_err(NumericExecutionError::Stack)?;
            binary(opcode, left, right, &mut arithmetic_throwable)?
        }
    };
    operands
        .push(result)
        .map_err(NumericExecutionError::Stack)?;
    Ok(JvmWorkReceipt::new(instruction.id()))
}

fn binary<F>(
    opcode: Opcode,
    left: JvmValue,
    right: JvmValue,
    throwable: &mut F,
) -> Result<JvmValue, NumericExecutionError>
where
    F: FnMut() -> JavaThrowable,
{
    macro_rules! ints {
        ($op:expr) => {{
            let (a, b) = (expect_i(opcode, left)?, expect_i(opcode, right)?);
            JvmValue::Int($op(a, b))
        }};
    }
    macro_rules! longs {
        ($op:expr) => {{
            let (a, b) = (expect_l(opcode, left)?, expect_l(opcode, right)?);
            JvmValue::Long($op(a, b))
        }};
    }
    macro_rules! floats { ($op:tt) => {{ let (a,b)=(f32::from_bits(expect_f(opcode,left)?),f32::from_bits(expect_f(opcode,right)?)); JvmValue::Float((a $op b).to_bits()) }}; }
    macro_rules! doubles { ($op:tt) => {{ let (a,b)=(f64::from_bits(expect_d(opcode,left)?),f64::from_bits(expect_d(opcode,right)?)); JvmValue::Double((a $op b).to_bits()) }}; }
    Ok(match opcode {
        Opcode::Iadd => ints!(i32::wrapping_add),
        Opcode::Isub => ints!(i32::wrapping_sub),
        Opcode::Imul => ints!(i32::wrapping_mul),
        Opcode::Ladd => longs!(i64::wrapping_add),
        Opcode::Lsub => longs!(i64::wrapping_sub),
        Opcode::Lmul => longs!(i64::wrapping_mul),
        Opcode::Fadd => floats!(+),
        Opcode::Fsub => floats!(-),
        Opcode::Fmul => floats!(*),
        Opcode::Fdiv => floats!(/),
        Opcode::Frem => floats!(%),
        Opcode::Dadd => doubles!(+),
        Opcode::Dsub => doubles!(-),
        Opcode::Dmul => doubles!(*),
        Opcode::Ddiv => doubles!(/),
        Opcode::Drem => doubles!(%),
        Opcode::Idiv | Opcode::Irem => {
            let (a, b) = (expect_i(opcode, left)?, expect_i(opcode, right)?);
            if b == 0 {
                return Err(NumericExecutionError::Arithmetic(Box::new(throwable())));
            }
            JvmValue::Int(if opcode == Opcode::Idiv {
                a.wrapping_div(b)
            } else {
                a.wrapping_rem(b)
            })
        }
        Opcode::Ldiv | Opcode::Lrem => {
            let (a, b) = (expect_l(opcode, left)?, expect_l(opcode, right)?);
            if b == 0 {
                return Err(NumericExecutionError::Arithmetic(Box::new(throwable())));
            }
            JvmValue::Long(if opcode == Opcode::Ldiv {
                a.wrapping_div(b)
            } else {
                a.wrapping_rem(b)
            })
        }
        Opcode::Iand => ints!(|a, b| a & b),
        Opcode::Ior => ints!(|a, b| a | b),
        Opcode::Ixor => ints!(|a, b| a ^ b),
        Opcode::Land => longs!(|a, b| a & b),
        Opcode::Lor => longs!(|a, b| a | b),
        Opcode::Lxor => longs!(|a, b| a ^ b),
        Opcode::Ishl => {
            let b = expect_i(opcode, right)? as u32 & 31;
            JvmValue::Int(expect_i(opcode, left)?.wrapping_shl(b))
        }
        Opcode::Ishr => {
            let b = expect_i(opcode, right)? as u32 & 31;
            JvmValue::Int(expect_i(opcode, left)?.wrapping_shr(b))
        }
        Opcode::Iushr => {
            let b = expect_i(opcode, right)? as u32 & 31;
            JvmValue::Int(((expect_i(opcode, left)? as u32) >> b) as i32)
        }
        Opcode::Lshl => {
            let b = expect_i(opcode, right)? as u32 & 63;
            JvmValue::Long(expect_l(opcode, left)?.wrapping_shl(b))
        }
        Opcode::Lshr => {
            let b = expect_i(opcode, right)? as u32 & 63;
            JvmValue::Long(expect_l(opcode, left)?.wrapping_shr(b))
        }
        Opcode::Lushr => {
            let b = expect_i(opcode, right)? as u32 & 63;
            JvmValue::Long(((expect_l(opcode, left)? as u64) >> b) as i64)
        }
        Opcode::Lcmp => JvmValue::Int(
            match expect_l(opcode, left)?.cmp(&expect_l(opcode, right)?) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        ),
        Opcode::Fcmpl | Opcode::Fcmpg => JvmValue::Int(float_cmp(
            f64::from(f32::from_bits(expect_f(opcode, left)?)),
            f64::from(f32::from_bits(expect_f(opcode, right)?)),
            opcode == Opcode::Fcmpg,
        )),
        Opcode::Dcmpl | Opcode::Dcmpg => JvmValue::Int(float_cmp(
            f64::from_bits(expect_d(opcode, left)?),
            f64::from_bits(expect_d(opcode, right)?),
            opcode == Opcode::Dcmpg,
        )),
        _ => return Err(NumericExecutionError::Category { opcode }),
    })
}

fn float_cmp(a: f64, b: f64, nan_high: bool) -> i32 {
    if a.is_nan() || b.is_nan() {
        if nan_high { 1 } else { -1 }
    } else if a > b {
        1
    } else if a < b {
        -1
    } else {
        0
    }
}
fn float_to_i32(v: f64) -> i32 {
    if v.is_nan() {
        0
    } else if v >= i32::MAX as f64 {
        i32::MAX
    } else if v <= i32::MIN as f64 {
        i32::MIN
    } else {
        v.trunc() as i32
    }
}
fn float_to_i64(v: f64) -> i64 {
    if v.is_nan() {
        0
    } else if v >= i64::MAX as f64 {
        i64::MAX
    } else if v <= i64::MIN as f64 {
        i64::MIN
    } else {
        v.trunc() as i64
    }
}
fn expect_i(opcode: Opcode, v: JvmValue) -> Result<i32, NumericExecutionError> {
    if let JvmValue::Int(x) = v {
        Ok(x)
    } else {
        Err(NumericExecutionError::Category { opcode })
    }
}
fn expect_l(opcode: Opcode, v: JvmValue) -> Result<i64, NumericExecutionError> {
    if let JvmValue::Long(x) = v {
        Ok(x)
    } else {
        Err(NumericExecutionError::Category { opcode })
    }
}
fn expect_f(opcode: Opcode, v: JvmValue) -> Result<u32, NumericExecutionError> {
    if let JvmValue::Float(x) = v {
        Ok(x)
    } else {
        Err(NumericExecutionError::Category { opcode })
    }
}
fn expect_d(opcode: Opcode, v: JvmValue) -> Result<u64, NumericExecutionError> {
    if let JvmValue::Double(x) = v {
        Ok(x)
    } else {
        Err(NumericExecutionError::Category { opcode })
    }
}
