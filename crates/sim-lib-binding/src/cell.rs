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
    slot: Arc<Mutex<BindingCellState>>,
}

/// The explicit lifecycle and mutation state of a [`BindingCell`].
#[derive(Clone, Debug)]
pub enum BindingCellState {
    /// The binding exists but has not received a value.
    Uninitialized,
    /// A mutable binding holding a value.
    Initialized(Value),
    /// A binding which has been removed and cannot be read.
    Deleted,
    /// A read-only binding holding a value.
    Immutable(Value),
    /// A binding which forwards reads and writes to another live cell.
    LiveAlias(BindingCell),
}

impl BindingCell {
    pub(crate) fn from_slot(name: Symbol, slot: Arc<Mutex<BindingCellState>>) -> Self {
        Self { name, slot }
    }

    /// Creates an uninitialized cell.
    pub fn uninitialized(name: Symbol) -> Self {
        Self::from_slot(name, Arc::new(Mutex::new(BindingCellState::Uninitialized)))
    }

    /// Creates an initialized mutable cell.
    pub fn initialized(name: Symbol, value: Value) -> Self {
        Self::from_slot(
            name,
            Arc::new(Mutex::new(BindingCellState::Initialized(value))),
        )
    }

    /// Creates an immutable initialized cell.
    pub fn immutable(name: Symbol, value: Value) -> Self {
        Self::from_slot(
            name,
            Arc::new(Mutex::new(BindingCellState::Immutable(value))),
        )
    }

    /// Creates a live alias which follows reads and writes to `target`.
    pub fn live_alias(name: Symbol, target: BindingCell) -> Self {
        Self::from_slot(
            name,
            Arc::new(Mutex::new(BindingCellState::LiveAlias(target))),
        )
    }

    /// Returns the binding name associated with this cell.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Reads the cell's current value.
    ///
    /// Errors if the captured slot is still uninitialized.
    pub fn get(&self) -> Result<Value> {
        let state = self
            .slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))?
            .clone();
        match state {
            BindingCellState::Initialized(value) | BindingCellState::Immutable(value) => Ok(value),
            BindingCellState::LiveAlias(target) => target.get(),
            BindingCellState::Uninitialized => Err(Error::Eval(format!(
                "binding cell {} is not initialized",
                self.name
            ))),
            BindingCellState::Deleted => Err(Error::Eval(format!(
                "binding cell {} is deleted",
                self.name
            ))),
        }
    }

    /// Replaces the cell's current value.
    pub fn set(&self, value: Value) -> Result<()> {
        let mut state = self
            .slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))?;
        match &mut *state {
            BindingCellState::LiveAlias(target) => target.set(value),
            BindingCellState::Immutable(_) => Err(Error::Eval(format!(
                "binding cell {} is immutable",
                self.name
            ))),
            BindingCellState::Deleted => Err(Error::Eval(format!(
                "binding cell {} is deleted",
                self.name
            ))),
            BindingCellState::Uninitialized | BindingCellState::Initialized(_) => {
                *state = BindingCellState::Initialized(value);
                Ok(())
            }
        }
    }

    /// Deletes this cell. A deleted cell cannot be read or reinitialized.
    pub fn delete(&self) -> Result<()> {
        let mut state = self
            .slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))?;
        if matches!(*state, BindingCellState::Immutable(_)) {
            return Err(Error::Eval(format!(
                "binding cell {} is immutable",
                self.name
            )));
        }
        *state = BindingCellState::Deleted;
        Ok(())
    }

    /// Returns a snapshot of the cell's current state.
    pub fn state(&self) -> Result<BindingCellState> {
        self.slot
            .lock()
            .map_err(|_| Error::Eval(format!("binding cell {} lock is poisoned", self.name)))
            .map(|state| state.clone())
    }
}
