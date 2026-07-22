//! Shared mutable cells for closed-over lexical bindings.

use std::sync::{Arc, Mutex};

use sim_kernel::{Error, Result, Symbol, Value};

/// A reference-shared mutable binding slot captured from a lexical scope.
///
/// Cloned cells point at the same slot, so writes through one handle are visible
/// through every other handle for the same lexical binding. Closure languages use
/// this shape for boxed upvalues and closed-over mutable locals.
#[derive(Clone, Debug)]
pub struct BindingCell {
    name: Symbol,
    slot: Arc<Mutex<Option<Value>>>,
}

impl BindingCell {
    pub(crate) fn from_slot(name: Symbol, slot: Arc<Mutex<Option<Value>>>) -> Self {
        Self { name, slot }
    }

    /// Returns the binding name associated with this cell.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Reads the cell's current value.
    ///
    /// Errors if the captured slot is still uninitialized.
    pub fn get(&self) -> Result<Value> {
        self.slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))?
            .clone()
            .ok_or_else(|| Error::Eval(format!("binding cell {} is not initialized", self.name)))
    }

    /// Replaces the cell's current value.
    pub fn set(&self, value: Value) -> Result<()> {
        *self
            .slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))? =
            Some(value);
        Ok(())
    }
}
