use std::collections::BTreeMap;

use sim_kernel::{Error, Result, Symbol, Value};

/// Flat local environment used by the Lua core eval policy.
#[derive(Clone, Debug, Default)]
pub struct LuaEnv {
    locals: BTreeMap<Symbol, Value>,
}

impl LuaEnv {
    /// Build an empty Lua local environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind or replace a Lua local value.
    pub fn define(&mut self, name: Symbol, value: Value) -> Option<Value> {
        self.locals.insert(name, value)
    }

    /// Return whether a Lua local is bound.
    pub fn contains(&self, name: &Symbol) -> bool {
        self.locals.contains_key(name)
    }

    /// Assign an existing Lua local.
    pub fn assign(&mut self, name: &Symbol, value: Value) -> Result<Value> {
        let Some(slot) = self.locals.get_mut(name) else {
            return Err(Error::UnknownSymbol {
                symbol: name.clone(),
            });
        };
        *slot = value.clone();
        Ok(value)
    }

    /// Look up a Lua local value.
    pub fn get(&self, name: &Symbol) -> Result<Value> {
        self.locals
            .get(name)
            .cloned()
            .ok_or_else(|| Error::UnknownSymbol {
                symbol: name.clone(),
            })
    }
}
