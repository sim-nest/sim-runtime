use sim_kernel::{Cx, Ref, Result, Symbol, Value};
use sim_lib_control::Coroutine;
use sim_lib_mutation::{MutableTable, mutable_table, mutable_table_value};

/// Builds a Lua coroutine that alternates between two ref lanes.
///
/// Lowers Lua coroutines onto the control organ's [`Coroutine`] rather than
/// defining bespoke coroutine semantics.
pub fn lua_coroutine(first: Vec<Ref>, second: Vec<Ref>) -> Coroutine {
    Coroutine::alternating(first, second)
}

/// Constructs a Lua table from keyed entries as a mutation-organ table value.
///
/// Lowers Lua tables onto the mutation organ's [`MutableTable`]; mutating the
/// result requires the standard mutate capability.
pub fn lua_table(cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
    mutable_table(cx, entries)
}

/// Borrows the [`MutableTable`] backing a Lua table value.
pub fn lua_table_value(value: &Value) -> Result<&MutableTable> {
    mutable_table_value(value)
}
