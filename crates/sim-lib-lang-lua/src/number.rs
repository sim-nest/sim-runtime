use sim_kernel::{Cx, Error, Expr, NumberLiteral, Result, Symbol, Value};

use crate::operator::LuaOp;

/// Lua numeric subtype used by the core operator layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LuaNumber {
    /// Integer subtype.
    Integer(i64),
    /// Floating-point subtype.
    Float(f64),
}

impl LuaNumber {
    fn as_f64(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn as_i64(self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(value),
            Self::Float(value) if value.fract() == 0.0 => {
                if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                    Some(value as i64)
                } else {
                    None
                }
            }
            Self::Float(_) => None,
        }
    }
}

/// Converts a SIM value to a Lua number, including Lua string-to-number coercion.
pub fn lua_number_from_value(cx: &mut Cx, value: &Value) -> Result<Option<LuaNumber>> {
    match value.object().as_expr(cx)? {
        Expr::Number(number) => Ok(number_from_literal(&number)),
        Expr::String(text) => Ok(number_from_text(&text)),
        _ => Ok(None),
    }
}

/// Builds a runtime value from a Lua integer.
pub fn lua_integer_value(cx: &mut Cx, value: i64) -> Result<Value> {
    cx.factory()
        .number_literal(lua_integer_domain(), value.to_string())
}

/// Builds a runtime value from a Lua float.
pub fn lua_float_value(cx: &mut Cx, value: f64) -> Result<Value> {
    if !value.is_finite() {
        return Err(Error::Eval("lua number result is not finite".to_owned()));
    }
    cx.factory()
        .number_literal(lua_float_domain(), canonical_float(value))
}

/// Applies Lua arithmetic, bitwise, comparison, and coercion rules.
pub fn lua_arith_or_compare(cx: &mut Cx, op: LuaOp, l: Value, r: Value) -> Result<Value> {
    match op {
        LuaOp::Concat => lua_concat(cx, &l, &r),
        LuaOp::Eq => cx.factory().bool(l == r),
        LuaOp::Lt | LuaOp::Le => lua_compare(cx, op, &l, &r),
        _ => {
            let left = required_number(cx, &l, op)?;
            let right = required_number(cx, &r, op)?;
            match op {
                LuaOp::Add => numeric_result(cx, left, right, |a, b| a + b, |a, b| a + b),
                LuaOp::Sub => numeric_result(cx, left, right, |a, b| a - b, |a, b| a - b),
                LuaOp::Mul => numeric_result(cx, left, right, |a, b| a * b, |a, b| a * b),
                LuaOp::FloatDiv => lua_float_value(cx, left.as_f64() / right.as_f64()),
                LuaOp::FloorDiv => floor_div(cx, left, right),
                LuaOp::Mod => modulo(cx, left, right),
                LuaOp::Pow => lua_float_value(cx, left.as_f64().powf(right.as_f64())),
                LuaOp::Band => bitwise(cx, left, right, |a, b| a & b),
                LuaOp::Bor => bitwise(cx, left, right, |a, b| a | b),
                LuaOp::Bxor => bitwise(cx, left, right, |a, b| a ^ b),
                LuaOp::Shl => bitwise(cx, left, right, |a, b| a.wrapping_shl(shift_count(b))),
                LuaOp::Shr => bitwise(cx, left, right, |a, b| a.wrapping_shr(shift_count(b))),
                LuaOp::Concat | LuaOp::Len | LuaOp::Eq | LuaOp::Lt | LuaOp::Le => unreachable!(),
            }
        }
    }
}

fn lua_compare(cx: &mut Cx, op: LuaOp, left: &Value, right: &Value) -> Result<Value> {
    if let (Some(left), Some(right)) = (
        lua_number_from_value(cx, left)?,
        lua_number_from_value(cx, right)?,
    ) {
        return cx.factory().bool(match op {
            LuaOp::Lt => left.as_f64() < right.as_f64(),
            LuaOp::Le => left.as_f64() <= right.as_f64(),
            _ => unreachable!(),
        });
    }
    let left = string_coercion(cx, left)?;
    let right = string_coercion(cx, right)?;
    cx.factory().bool(match op {
        LuaOp::Lt => left < right,
        LuaOp::Le => left <= right,
        _ => unreachable!(),
    })
}

fn lua_concat(cx: &mut Cx, left: &Value, right: &Value) -> Result<Value> {
    let left = string_coercion(cx, left)?;
    let right = string_coercion(cx, right)?;
    cx.factory().string(format!("{left}{right}"))
}

fn required_number(cx: &mut Cx, value: &Value, op: LuaOp) -> Result<LuaNumber> {
    lua_number_from_value(cx, value)?.ok_or_else(|| {
        Error::Eval(format!(
            "lua operator {} requires numeric operands",
            op.name()
        ))
    })
}

fn numeric_result(
    cx: &mut Cx,
    left: LuaNumber,
    right: LuaNumber,
    integer_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value> {
    match (left, right) {
        (LuaNumber::Integer(left), LuaNumber::Integer(right)) => {
            lua_integer_value(cx, integer_op(left, right))
        }
        _ => lua_float_value(cx, float_op(left.as_f64(), right.as_f64())),
    }
}

fn floor_div(cx: &mut Cx, left: LuaNumber, right: LuaNumber) -> Result<Value> {
    if right.as_f64() == 0.0 {
        return Err(Error::Eval("lua floor division by zero".to_owned()));
    }
    match (left, right) {
        (LuaNumber::Integer(left), LuaNumber::Integer(right)) => {
            lua_integer_value(cx, (left as f64 / right as f64).floor() as i64)
        }
        _ => lua_float_value(cx, (left.as_f64() / right.as_f64()).floor()),
    }
}

fn modulo(cx: &mut Cx, left: LuaNumber, right: LuaNumber) -> Result<Value> {
    if right.as_f64() == 0.0 {
        return Err(Error::Eval("lua modulo by zero".to_owned()));
    }
    match (left, right) {
        (LuaNumber::Integer(left), LuaNumber::Integer(right)) => {
            lua_integer_value(cx, left.rem_euclid(right))
        }
        _ => {
            let divisor = right.as_f64();
            lua_float_value(
                cx,
                left.as_f64() - (left.as_f64() / divisor).floor() * divisor,
            )
        }
    }
}

fn bitwise(
    cx: &mut Cx,
    left: LuaNumber,
    right: LuaNumber,
    op: fn(i64, i64) -> i64,
) -> Result<Value> {
    let left = left
        .as_i64()
        .ok_or_else(|| Error::Eval("lua bitwise operand must be an integer".to_owned()))?;
    let right = right
        .as_i64()
        .ok_or_else(|| Error::Eval("lua bitwise operand must be an integer".to_owned()))?;
    lua_integer_value(cx, op(left, right))
}

fn shift_count(value: i64) -> u32 {
    value.clamp(0, 63) as u32
}

fn string_coercion(cx: &mut Cx, value: &Value) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        Expr::Number(number) => Ok(number.canonical),
        other => Err(Error::Eval(format!(
            "lua operator cannot coerce {other:?} to string"
        ))),
    }
}

fn number_from_literal(number: &NumberLiteral) -> Option<LuaNumber> {
    if let Ok(value) = number.canonical.parse::<i64>() {
        return Some(LuaNumber::Integer(value));
    }
    let value = number.canonical.parse::<f64>().ok()?;
    value.is_finite().then_some(LuaNumber::Float(value))
}

fn number_from_text(text: &str) -> Option<LuaNumber> {
    let text = text.trim();
    if let Ok(value) = text.parse::<i64>() {
        return Some(LuaNumber::Integer(value));
    }
    let value = text.parse::<f64>().ok()?;
    value.is_finite().then_some(LuaNumber::Float(value))
}

fn canonical_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

fn lua_integer_domain() -> Symbol {
    Symbol::qualified("lua", "integer")
}

fn lua_float_domain() -> Symbol {
    Symbol::qualified("lua", "float")
}
