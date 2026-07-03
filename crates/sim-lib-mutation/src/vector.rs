use std::sync::{Arc, RwLock};

use sim_kernel::{Cx, Error, Expr, Object, ObjectCompat, Result, Value};

use crate::standard_mutate_capability;

/// A growable, index-addressed vector of [`Value`]s with in-place mutation.
///
/// Unlike the persistent sequences in `sim-lib-sequence`, writes here mutate the
/// existing object in place; [`set`](MutableVector::set) and
/// [`push`](MutableVector::push) require [`standard_mutate_capability`], while
/// reads are always allowed. It renders to an [`Expr::Vector`] for inspection.
#[sim_citizen_derive::non_citizen(
    reason = "mutable vector handle; reconstruct from vector entries plus mutation policy",
    kind = "handle",
    descriptor = "core/Expr"
)]
pub struct MutableVector {
    items: RwLock<Vec<Value>>,
}

impl MutableVector {
    /// Create a vector seeded with `items`.
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: RwLock::new(items),
        }
    }

    /// Return the current element count. Reads need no capability.
    pub fn len(&self) -> Result<usize> {
        Ok(self
            .items
            .read()
            .map_err(|_| Error::PoisonedLock("mutation vector"))?
            .len())
    }

    /// Return whether the vector currently has no elements.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Return a clone of the element at `index`, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Result<Option<Value>> {
        Ok(self
            .items
            .read()
            .map_err(|_| Error::PoisonedLock("mutation vector"))?
            .get(index)
            .cloned())
    }

    /// Overwrite the element at `index`, requiring [`standard_mutate_capability`].
    ///
    /// Errors if `index` is out of bounds.
    pub fn set(&self, cx: &mut Cx, index: usize, value: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        let mut items = self
            .items
            .write()
            .map_err(|_| Error::PoisonedLock("mutation vector"))?;
        let Some(slot) = items.get_mut(index) else {
            return Err(Error::Eval(format!(
                "mutable vector index {index} out of bounds"
            )));
        };
        *slot = value;
        Ok(())
    }

    /// Append `value` to the end, requiring [`standard_mutate_capability`].
    pub fn push(&self, cx: &mut Cx, value: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        self.items
            .write()
            .map_err(|_| Error::PoisonedLock("mutation vector"))?
            .push(value);
        Ok(())
    }

    /// Return a snapshot clone of the current elements. Reads need no capability.
    pub fn to_vec(&self) -> Result<Vec<Value>> {
        Ok(self
            .items
            .read()
            .map_err(|_| Error::PoisonedLock("mutation vector"))?
            .clone())
    }
}

impl Object for MutableVector {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<mutation-vector {}>", self.len()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MutableVector {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Vector(
            self.to_vec()?
                .iter()
                .map(|value| value.object().as_expr(cx))
                .collect::<Result<Vec<_>>>()?,
        ))
    }
}

/// Construct a [`MutableVector`] from `items` and wrap it as a runtime [`Value`].
pub fn mutable_vector(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    cx.factory().opaque(Arc::new(MutableVector::new(items)))
}

/// Build a [`MutableVector`] by copying the elements of a vector-shaped `value`.
///
/// The source is read through its [`Expr::Vector`] form, so the resulting
/// mutable vector is an independent copy; errors if `value` is not vector-shaped.
pub fn mutable_vector_from_value(cx: &mut Cx, value: &Value) -> Result<Value> {
    let expr = value.object().as_expr(cx)?;
    let Expr::Vector(items) = expr else {
        return Err(Error::TypeMismatch {
            expected: "vector expression",
            found: "non-vector",
        });
    };
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        values.push(cx.factory().expr(item)?);
    }
    mutable_vector(cx, values)
}

/// Borrow the [`MutableVector`] behind `value`, or error if it is not one.
pub fn mutable_vector_value(value: &Value) -> Result<&MutableVector> {
    value
        .object()
        .downcast_ref::<MutableVector>()
        .ok_or(Error::TypeMismatch {
            expected: "mutable vector",
            found: "non-vector",
        })
}
