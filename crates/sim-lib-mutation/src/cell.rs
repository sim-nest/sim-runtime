use std::sync::{Arc, RwLock};

use sim_kernel::{Cx, Error, Object, ObjectCompat, Result, Value};

use crate::standard_mutate_capability;

/// A shared, mutable cell holding a single [`Value`].
///
/// The cell is the base mutation handle: a cloneable, reference-counted slot
/// whose [`set`](Cell::set) requires [`standard_mutate_capability`]. Reads are
/// always allowed; writes fail closed without the capability.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_mutation::{Cell, standard_mutate_capability};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let old = cx.factory().string("old".to_owned()).unwrap();
/// let cell = Cell::new(old);
///
/// // Writing fails closed until the mutate capability is granted.
/// let new = cx.factory().string("new".to_owned()).unwrap();
/// assert!(cell.set(&mut cx, new).is_err());
///
/// cx.grant(standard_mutate_capability());
/// let new = cx.factory().string("new".to_owned()).unwrap();
/// cell.set(&mut cx, new).unwrap();
/// ```
#[sim_citizen_derive::non_citizen(
    reason = "mutable cell handle; reconstruct from the current value plus mutation policy",
    kind = "handle",
    descriptor = "core/Expr"
)]
#[derive(Clone)]
pub struct Cell {
    value: Arc<RwLock<Value>>,
}

impl Cell {
    /// Create a cell initialized to `value`.
    pub fn new(value: Value) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
        }
    }

    /// Return a clone of the current value. Reads need no capability.
    pub fn get(&self) -> Result<Value> {
        Ok(self
            .value
            .read()
            .map_err(|_| Error::PoisonedLock("mutation cell"))?
            .clone())
    }

    /// Overwrite the cell with `value`, requiring [`standard_mutate_capability`].
    pub fn set(&self, cx: &mut Cx, value: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        *self
            .value
            .write()
            .map_err(|_| Error::PoisonedLock("mutation cell"))? = value;
        Ok(())
    }
}

impl Object for Cell {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<mutation-cell>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for Cell {}

/// A mutable box: a single-slot mutation handle over a [`Cell`].
///
/// Distinct runtime type from [`Cell`] so boxes and cells present as separate
/// objects, but with the same capability-gated read/write behavior.
#[sim_citizen_derive::non_citizen(
    reason = "mutable box handle; reconstruct from the current value plus mutation policy",
    kind = "handle",
    descriptor = "core/Expr"
)]
#[derive(Clone)]
pub struct MutableBox {
    cell: Cell,
}

impl MutableBox {
    /// Create a box initialized to `value`.
    pub fn new(value: Value) -> Self {
        Self {
            cell: Cell::new(value),
        }
    }

    /// Return a clone of the boxed value. Reads need no capability.
    pub fn get(&self) -> Result<Value> {
        self.cell.get()
    }

    /// Overwrite the boxed value, requiring [`standard_mutate_capability`].
    pub fn set(&self, cx: &mut Cx, value: Value) -> Result<()> {
        self.cell.set(cx, value)
    }
}

impl Object for MutableBox {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<mutation-box>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MutableBox {}

/// Construct a [`Cell`] initialized to `value` and wrap it as a runtime [`Value`].
pub fn cell_value(cx: &mut Cx, value: Value) -> Result<Value> {
    cx.factory().opaque(Arc::new(Cell::new(value)))
}

/// Construct a [`MutableBox`] initialized to `value` and wrap it as a runtime [`Value`].
pub fn mutable_box_value(cx: &mut Cx, value: Value) -> Result<Value> {
    cx.factory().opaque(Arc::new(MutableBox::new(value)))
}
