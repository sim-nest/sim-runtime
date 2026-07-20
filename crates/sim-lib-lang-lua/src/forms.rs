use sim_kernel::{Error, Expr, Result, Symbol};

use crate::LuaOp;

#[derive(Clone, Copy)]
pub(crate) enum LuaForm {
    Chunk,
    Block,
    Local,
    LocalValues,
    Assign,
    If,
    Call,
    Closure,
    Varargs,
    Return,
    Break,
    NumericFor,
    GenericFor,
    Stdlib,
    Table,
    Get,
    RawGet,
    RawSet,
    Len,
    Binary(LuaOp),
}

pub(crate) fn lua_form(expr: &Expr) -> Option<(LuaForm, &[Expr])> {
    let Expr::List(items) = expr else {
        return None;
    };
    let (head, args) = items.split_first()?;
    let Expr::Symbol(symbol) = head else {
        return None;
    };
    lua_form_symbol(symbol).map(|form| (form, args))
}

pub(crate) fn required_head<'a>(args: &'a [Expr], context: &str) -> Result<(&'a Expr, &'a [Expr])> {
    args.split_first()
        .ok_or_else(|| Error::Eval(format!("{context} requires a target")))
}

pub(crate) fn binding_symbol(expr: &Expr, context: &str) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) | Expr::Local(symbol) => Ok(symbol.clone()),
        _ => Err(Error::Eval(format!(
            "{context} requires a symbol binding target"
        ))),
    }
}

pub(crate) fn symbol_list(expr: &Expr, context: &str) -> Result<Vec<Symbol>> {
    let items = match expr {
        Expr::List(items) | Expr::Vector(items) => items,
        _ => {
            return Err(Error::Eval(format!("{context} requires a list of symbols")));
        }
    };
    items
        .iter()
        .map(|expr| binding_symbol(expr, context))
        .collect()
}

pub(crate) fn bool_literal(expr: &Expr, context: &str) -> Result<bool> {
    match expr {
        Expr::Bool(value) => Ok(*value),
        _ => Err(Error::Eval(format!("{context} must be a boolean literal"))),
    }
}

fn lua_form_symbol(symbol: &Symbol) -> Option<LuaForm> {
    if !matches!(
        symbol.namespace.as_deref(),
        Some("lua") | Some("lua/core") | None
    ) {
        return None;
    }
    match symbol.name.as_ref() {
        "chunk" => Some(LuaForm::Chunk),
        "block" => Some(LuaForm::Block),
        "local" => Some(LuaForm::Local),
        "local-values" => Some(LuaForm::LocalValues),
        "assign" => Some(LuaForm::Assign),
        "if" => Some(LuaForm::If),
        "call" => Some(LuaForm::Call),
        "closure" | "function" => Some(LuaForm::Closure),
        "varargs" => Some(LuaForm::Varargs),
        "return" => Some(LuaForm::Return),
        "break" => Some(LuaForm::Break),
        "for-num" => Some(LuaForm::NumericFor),
        "for-in" => Some(LuaForm::GenericFor),
        "stdlib" => Some(LuaForm::Stdlib),
        "table" => Some(LuaForm::Table),
        "get" => Some(LuaForm::Get),
        "rawget" => Some(LuaForm::RawGet),
        "rawset" => Some(LuaForm::RawSet),
        "len" => Some(LuaForm::Len),
        "add" => Some(LuaForm::Binary(LuaOp::Add)),
        "sub" => Some(LuaForm::Binary(LuaOp::Sub)),
        "mul" => Some(LuaForm::Binary(LuaOp::Mul)),
        "div" => Some(LuaForm::Binary(LuaOp::FloatDiv)),
        "floordiv" => Some(LuaForm::Binary(LuaOp::FloorDiv)),
        "mod" => Some(LuaForm::Binary(LuaOp::Mod)),
        "pow" => Some(LuaForm::Binary(LuaOp::Pow)),
        "band" => Some(LuaForm::Binary(LuaOp::Band)),
        "bor" => Some(LuaForm::Binary(LuaOp::Bor)),
        "bxor" => Some(LuaForm::Binary(LuaOp::Bxor)),
        "shl" => Some(LuaForm::Binary(LuaOp::Shl)),
        "shr" => Some(LuaForm::Binary(LuaOp::Shr)),
        "concat" => Some(LuaForm::Binary(LuaOp::Concat)),
        "eq" => Some(LuaForm::Binary(LuaOp::Eq)),
        "lt" => Some(LuaForm::Binary(LuaOp::Lt)),
        "le" => Some(LuaForm::Binary(LuaOp::Le)),
        _ => None,
    }
}
