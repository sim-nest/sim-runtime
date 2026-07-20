use std::sync::{Arc, RwLock};

use sim_kernel::{Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value};
use sim_lib_mutation::{
    MutableRuntimeTable, RuntimeKey, RuntimeKeyPolicy, standard_mutate_capability,
};
use sim_lib_sequence::RuntimeIndexSource;

use crate::number::lua_number_from_value;

/// Lua table key policy over the shared runtime-keyed mutation table.
#[derive(Clone, Copy, Debug, Default)]
pub struct LuaTablePolicy;

impl RuntimeKeyPolicy for LuaTablePolicy {
    fn key_for(&self, cx: &mut Cx, value: &Value) -> Result<Option<RuntimeKey>> {
        match value.object().as_expr(cx)? {
            Expr::Nil => Ok(None),
            Expr::Number(_) => lua_number_key(cx, value),
            Expr::Bool(value) => Ok(Some(RuntimeKey::Bool(value))),
            Expr::String(value) => Ok(Some(RuntimeKey::Str(value))),
            Expr::Symbol(symbol) => Ok(Some(RuntimeKey::Symbol(symbol))),
            _ => RuntimeKey::from_value(cx, value),
        }
    }
}

/// Lua table handle backed by the mutation organ's runtime-keyed table.
pub struct LuaTable {
    entries: MutableRuntimeTable<LuaTablePolicy>,
    metatable: RwLock<Option<Value>>,
}

impl LuaTable {
    /// Builds a Lua table from already-evaluated key/value entries.
    pub fn new(cx: &mut Cx, entries: Vec<(Value, Value)>) -> Result<Self> {
        Ok(Self {
            entries: MutableRuntimeTable::with_entries(cx, LuaTablePolicy, entries)?,
            metatable: RwLock::new(None),
        })
    }

    /// Reads a raw entry without consulting `__index`.
    pub fn raw_get(&self, cx: &mut Cx, key: &Value) -> Result<Option<Value>> {
        self.entries.get(cx, key)
    }

    /// Writes a raw entry after checking mutation authority.
    pub fn raw_set(&self, cx: &mut Cx, key: Value, value: Value) -> Result<()> {
        self.entries.set(cx, key, value)
    }

    /// Reads a symbol-keyed raw entry.
    pub fn get_symbol(&self, cx: &mut Cx, key: Symbol) -> Result<Option<Value>> {
        let key = cx.factory().symbol(key)?;
        self.raw_get(cx, &key)
    }

    /// Writes a symbol-keyed raw entry.
    pub fn set_symbol(&self, cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        let key = cx.factory().symbol(key)?;
        self.raw_set(cx, key, value)
    }

    /// Installs the table's metatable after checking mutation authority.
    pub fn set_metatable(&self, cx: &mut Cx, metatable: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        *self
            .metatable
            .write()
            .map_err(|_| Error::PoisonedLock("lua table metatable"))? = Some(metatable);
        Ok(())
    }

    /// Returns the current metatable, if any.
    pub fn metatable(&self) -> Result<Option<Value>> {
        Ok(self
            .metatable
            .read()
            .map_err(|_| Error::PoisonedLock("lua table metatable"))?
            .clone())
    }

    /// Returns the Lua length border by walking contiguous integer keys from 1.
    pub fn len_border(&self, _cx: &mut Cx) -> Result<i64> {
        let mut index = 1_i64;
        loop {
            if self
                .entries
                .get_runtime_key(&RuntimeKey::Integer(index))?
                .is_none()
            {
                return Ok(index - 1);
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::Eval("lua table length overflow".to_owned()))?;
            if index > 1_000_000 {
                return Err(Error::Eval(
                    "lua table length exceeded bounded scan".to_owned(),
                ));
            }
        }
    }

    /// Returns entries in deterministic key order.
    pub fn entries_in_key_order(&self) -> Result<Vec<(RuntimeKey, Value)>> {
        self.entries.entries_in_key_order()
    }
}

impl RuntimeIndexSource for LuaTable {
    fn value_at_runtime_index(&self, _cx: &mut Cx, index: i64) -> Result<Option<Value>> {
        self.entries.get_runtime_key(&RuntimeKey::Integer(index))
    }
}

impl Object for LuaTable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-table {}>", self.entries.len()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaTable {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Map(
            self.entries_in_key_order()?
                .into_iter()
                .map(|(key, value)| Ok((key.as_expr(), value.object().as_expr(cx)?)))
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }
}

/// Constructs a Lua table from value-keyed entries.
pub fn lua_table_from_values(cx: &mut Cx, entries: Vec<(Value, Value)>) -> Result<Value> {
    let table = LuaTable::new(cx, entries)?;
    cx.factory().opaque(Arc::new(table))
}

/// Constructs a Lua table from symbol-keyed entries.
pub fn lua_table(cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
    let mut value_entries = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        value_entries.push((cx.factory().symbol(key)?, value));
    }
    lua_table_from_values(cx, value_entries)
}

/// Borrows the Lua table behind `value`.
pub fn lua_table_value(value: &Value) -> Result<&LuaTable> {
    value
        .object()
        .downcast_ref::<LuaTable>()
        .ok_or(Error::TypeMismatch {
            expected: "lua table",
            found: "non-table",
        })
}

/// Performs a raw Lua table read without consulting `__index`.
pub fn lua_rawget(cx: &mut Cx, table: &Value, key: &Value) -> Result<Option<Value>> {
    lua_table_value(table)?.raw_get(cx, key)
}

/// Performs a raw Lua table write without consulting `__newindex`.
pub fn lua_rawset(cx: &mut Cx, table: &Value, key: Value, value: Value) -> Result<()> {
    lua_table_value(table)?.raw_set(cx, key, value)
}

/// Installs a Lua table metatable.
pub fn lua_set_metatable(cx: &mut Cx, table: &Value, metatable: Value) -> Result<()> {
    lua_table_value(table)?.set_metatable(cx, metatable)
}

fn lua_number_key(cx: &mut Cx, value: &Value) -> Result<Option<RuntimeKey>> {
    let Some(number) = lua_number_from_value(cx, value)? else {
        return Ok(None);
    };
    match number {
        crate::number::LuaNumber::Integer(value) => Ok(Some(RuntimeKey::Integer(value))),
        crate::number::LuaNumber::Float(value) if value.is_nan() => Ok(None),
        crate::number::LuaNumber::Float(value)
            if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 =>
        {
            Ok(Some(RuntimeKey::Integer(value as i64)))
        }
        crate::number::LuaNumber::Float(value) => Ok(Some(RuntimeKey::FloatBits(value.to_bits()))),
    }
}
