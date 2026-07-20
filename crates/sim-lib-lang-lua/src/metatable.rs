use sim_kernel::{Args, Cx, Result, Symbol, Value};
use sim_lib_dispatch::{MetaObjectProtocol, meta_index};

use crate::table::{lua_rawget, lua_table_value};

/// Stable Lua metatable slot for indexed reads.
pub fn lua_index_slot() -> Symbol {
    Symbol::new("__index")
}

/// Performs Lua indexed read with the shared metaobject protocol.
pub fn lua_get(cx: &mut Cx, receiver: &Value, key: &Value) -> Result<Option<Value>> {
    meta_index(cx, &LuaMetaProtocol, receiver, key, &lua_index_slot())
}

/// Reads a raw metamethod from a value's metatable.
pub fn lua_metamethod(cx: &mut Cx, value: &Value, slot: &Symbol) -> Result<Option<Value>> {
    let table = match lua_table_value(value) {
        Ok(table) => table,
        Err(_) => return Ok(None),
    };
    let Some(metatable) = table.metatable()? else {
        return Ok(None);
    };
    let slot = cx.factory().string(slot.name.to_string())?;
    lua_rawget(cx, &metatable, &slot)
}

struct LuaMetaProtocol;

impl MetaObjectProtocol for LuaMetaProtocol {
    fn raw_get(&self, cx: &mut Cx, value: &Value, key: &Value) -> Result<Option<Value>> {
        let Ok(table) = lua_table_value(value) else {
            return Ok(None);
        };
        table.raw_get(cx, key)
    }

    fn get_meta(&self, cx: &mut Cx, value: &Value, slot: &Symbol) -> Result<Option<Value>> {
        lua_metamethod(cx, value, slot)
    }

    fn apply_meta(
        &self,
        cx: &mut Cx,
        receiver: &Value,
        key: &Value,
        index_slot: &Symbol,
        meta_value: &Value,
    ) -> Result<Option<Value>> {
        if lua_table_value(meta_value).is_ok() {
            return meta_index(cx, self, meta_value, key, index_slot);
        }
        cx.call_value(
            meta_value.clone(),
            Args::new(vec![receiver.clone(), key.clone()]),
        )
        .map(Some)
    }
}
