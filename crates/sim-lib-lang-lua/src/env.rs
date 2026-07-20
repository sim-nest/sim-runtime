use sim_kernel::{Result, Symbol, Value};
use sim_lib_binding::{BindingCell, LexicalEnv};

/// Lexical local environment used by the Lua core eval policy.
#[derive(Clone, Debug, Default)]
pub struct LuaEnv {
    lexical: LexicalEnv,
}

impl LuaEnv {
    /// Build an empty Lua local environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a nested scope whose lookups fall through to this one.
    pub fn child(&self) -> Self {
        Self {
            lexical: self.lexical.child(),
        }
    }

    /// Bind a Lua local value in the current frame.
    pub fn define(&mut self, name: Symbol, value: Value) -> Result<()> {
        self.lexical.define(name, value)
    }

    /// Return whether a Lua local is bound.
    pub fn contains(&self, name: &Symbol) -> bool {
        self.lexical.lookup(name).is_ok()
    }

    /// Assign an existing Lua local.
    pub fn assign(&mut self, name: &Symbol, value: Value) -> Result<Value> {
        self.capture(name)?.set(value.clone())?;
        Ok(value)
    }

    /// Look up a Lua local value.
    pub fn get(&self, name: &Symbol) -> Result<Value> {
        self.lexical.lookup(name)
    }

    /// Capture an existing Lua local as a shared upvalue cell.
    pub fn capture(&self, name: &Symbol) -> Result<BindingCell> {
        self.lexical.capture_cell(name)
    }
}
