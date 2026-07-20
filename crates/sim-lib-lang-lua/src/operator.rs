use sim_kernel::{Args, Cx, Expr, Result, Symbol, Value};

use crate::{
    LuaEnv,
    metatable::lua_metamethod,
    number::{lua_arith_or_compare, lua_integer_value},
    table::lua_table_value,
};

/// Lua core binary and unary operators covered by the current profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuaOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Floating-point division.
    FloatDiv,
    /// Floor division.
    FloorDiv,
    /// Modulo.
    Mod,
    /// Exponentiation.
    Pow,
    /// Bitwise and.
    Band,
    /// Bitwise or.
    Bor,
    /// Bitwise xor.
    Bxor,
    /// Bitwise left shift.
    Shl,
    /// Bitwise right shift.
    Shr,
    /// Concatenation.
    Concat,
    /// Length.
    Len,
    /// Equality.
    Eq,
    /// Less-than.
    Lt,
    /// Less-than-or-equal.
    Le,
}

impl LuaOp {
    /// Returns the Lua source-level operator spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::FloatDiv => "/",
            Self::FloorDiv => "//",
            Self::Mod => "%",
            Self::Pow => "^",
            Self::Band => "&",
            Self::Bor => "|",
            Self::Bxor => "~",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Concat => "..",
            Self::Len => "#",
            Self::Eq => "==",
            Self::Lt => "<",
            Self::Le => "<=",
        }
    }

    /// Returns the Lua metamethod slot for this operator.
    pub fn metamethod_slot(self) -> Symbol {
        Symbol::new(match self {
            Self::Add => "__add",
            Self::Sub => "__sub",
            Self::Mul => "__mul",
            Self::FloatDiv => "__div",
            Self::FloorDiv => "__idiv",
            Self::Mod => "__mod",
            Self::Pow => "__pow",
            Self::Band => "__band",
            Self::Bor => "__bor",
            Self::Bxor => "__bxor",
            Self::Shl => "__shl",
            Self::Shr => "__shr",
            Self::Concat => "__concat",
            Self::Len => "__len",
            Self::Eq => "__eq",
            Self::Lt => "__lt",
            Self::Le => "__le",
        })
    }
}

/// Applies a Lua binary operator using metamethods before primitive behavior.
pub fn lua_binary(
    cx: &mut Cx,
    _env: &mut LuaEnv,
    op: LuaOp,
    left: Value,
    right: Value,
) -> Result<Value> {
    if let Some(value) = try_binary_metamethod(cx, op, &left, &right)? {
        return Ok(value);
    }
    lua_arith_or_compare(cx, op, left, right)
}

/// Applies Lua length over strings and tables, with `__len` fallback.
pub fn lua_len(cx: &mut Cx, _env: &mut LuaEnv, value: Value) -> Result<Value> {
    if let Some(method) = lua_metamethod(cx, &value, &LuaOp::Len.metamethod_slot())? {
        return cx.call_value(method, Args::new(vec![value]));
    }
    match value.object().as_expr(cx)? {
        Expr::String(text) => lua_integer_value(cx, text.chars().count() as i64),
        _ => {
            let table = lua_table_value(&value)?;
            let len = table.len_border(cx)?;
            lua_integer_value(cx, len)
        }
    }
}

fn try_binary_metamethod(
    cx: &mut Cx,
    op: LuaOp,
    left: &Value,
    right: &Value,
) -> Result<Option<Value>> {
    let slot = op.metamethod_slot();
    let method = lua_metamethod(cx, left, &slot)?.or(lua_metamethod(cx, right, &slot)?);
    let Some(method) = method else {
        return Ok(None);
    };
    cx.call_value(method, Args::new(vec![left.clone(), right.clone()]))
        .map(Some)
}
