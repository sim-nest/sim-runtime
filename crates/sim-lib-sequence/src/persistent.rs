use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value, force_list_to_vec};

#[sim_citizen_derive::non_citizen(
    reason = "persistent vector wrapper; canonical form is native Expr::Vector data",
    kind = "marker"
)]
/// Immutable, shareable vector object backed by a shared slice.
///
/// A persistent sequence container: construction never mutates inputs, and the
/// canonical form is native [`Expr::Vector`] data per the kernel object
/// contract.
#[derive(Clone, Debug)]
pub struct PersistentVector {
    items: Arc<[Value]>,
}

impl PersistentVector {
    /// Build a persistent vector from the given elements.
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: Arc::from(items),
        }
    }

    /// Borrow the vector elements in order.
    pub fn items(&self) -> &[Value] {
        &self.items
    }
}

impl Object for PersistentVector {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<sequence-vector {}>", self.items.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for PersistentVector {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Vector(
            self.items
                .iter()
                .map(|value| value.object().as_expr(cx))
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!self.items.is_empty())
    }
}

#[sim_citizen_derive::non_citizen(
    reason = "persistent set wrapper; canonical form is native Expr::Set data",
    kind = "marker"
)]
/// Immutable, shareable set object holding canonically distinct elements.
///
/// A persistent sequence container that deduplicates by canonical expression
/// equality; the canonical form is native [`Expr::Set`] data per the kernel
/// object contract.
#[derive(Clone, Debug)]
pub struct PersistentSet {
    items: Arc<[Value]>,
}

impl PersistentSet {
    /// Build a persistent set, dropping canonically duplicate elements.
    pub fn new(cx: &mut Cx, items: Vec<Value>) -> Result<Self> {
        let mut unique = Vec::new();
        for item in items {
            if !contains_canonical(cx, &unique, &item)? {
                unique.push(item);
            }
        }
        Ok(Self {
            items: Arc::from(unique),
        })
    }

    /// Borrow the set elements in insertion order.
    pub fn items(&self) -> &[Value] {
        &self.items
    }
}

impl Object for PersistentSet {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<sequence-set {}>", self.items.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for PersistentSet {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Set(
            self.items
                .iter()
                .map(|value| value.object().as_expr(cx))
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!self.items.is_empty())
    }
}

/// Construct an immutable list [`Value`] from the given elements.
pub fn persistent_list(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    cx.new_list(items)
}

/// Return a new list with `item` appended; the input list is unchanged.
pub fn persistent_list_push(cx: &mut Cx, list: &Value, item: Value) -> Result<Value> {
    let mut items = list_items(cx, list)?;
    items.push(item);
    cx.new_list(items)
}

/// Construct a [`PersistentVector`] as a runtime [`Value`].
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_sequence::{persistent_vector, persistent_vector_push};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let one = cx.factory().number_literal(Symbol::qualified("test", "u64"), "1".into())?;
/// let two = cx.factory().number_literal(Symbol::qualified("test", "u64"), "2".into())?;
///
/// let base = persistent_vector(&mut cx, vec![one])?;
/// // Push returns a new vector; `base` keeps its single element.
/// let _grown = persistent_vector_push(&mut cx, &base, two)?;
/// # Ok::<(), sim_kernel::Error>(())
/// ```
pub fn persistent_vector(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    cx.factory().opaque(Arc::new(PersistentVector::new(items)))
}

/// Return a new vector with `item` appended; the input vector is unchanged.
pub fn persistent_vector_push(cx: &mut Cx, vector: &Value, item: Value) -> Result<Value> {
    let vector = vector_value(vector)?;
    let mut items = vector.items().to_vec();
    items.push(item);
    persistent_vector(cx, items)
}

/// Construct a [`PersistentSet`] as a runtime [`Value`], deduplicating elements.
pub fn persistent_set(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    let set = PersistentSet::new(cx, items)?;
    cx.factory().opaque(Arc::new(set))
}

/// Return a new set with `item` inserted if canonically absent; input unchanged.
pub fn persistent_set_insert(cx: &mut Cx, set: &Value, item: Value) -> Result<Value> {
    let set = set_value(set)?;
    let mut items = set.items().to_vec();
    if !contains_canonical(cx, &items, &item)? {
        items.push(item);
    }
    persistent_set(cx, items)
}

/// Construct an immutable map (table) [`Value`] from key/value entries.
pub fn persistent_map(cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
    cx.new_table(entries)
}

/// Return a new map with `key` bound to `value`; the input map is unchanged.
///
/// Replaces an existing binding for `key` or appends a fresh one.
pub fn persistent_map_assoc(cx: &mut Cx, map: &Value, key: Symbol, value: Value) -> Result<Value> {
    let table = map.object().as_table_impl().ok_or(Error::TypeMismatch {
        expected: "table",
        found: "non-table",
    })?;
    let mut entries = table.entries(cx)?;
    match entries.iter_mut().find(|(candidate, _)| *candidate == key) {
        Some((_, slot)) => *slot = value,
        None => entries.push((key, value)),
    }
    cx.new_table(entries)
}

fn list_items(cx: &mut Cx, value: &Value) -> Result<Vec<Value>> {
    let list = value.object().as_list().ok_or(Error::TypeMismatch {
        expected: "list",
        found: "non-list",
    })?;
    force_list_to_vec(cx, list, "sequence persistent list")
}

fn vector_value(value: &Value) -> Result<&PersistentVector> {
    value
        .object()
        .downcast_ref::<PersistentVector>()
        .ok_or(Error::TypeMismatch {
            expected: "sequence vector",
            found: "non-vector",
        })
}

fn set_value(value: &Value) -> Result<&PersistentSet> {
    value
        .object()
        .downcast_ref::<PersistentSet>()
        .ok_or(Error::TypeMismatch {
            expected: "sequence set",
            found: "non-set",
        })
}

fn contains_canonical(cx: &mut Cx, values: &[Value], candidate: &Value) -> Result<bool> {
    let candidate = candidate.object().as_expr(cx)?;
    for value in values {
        if value.object().as_expr(cx)?.canonical_eq(&candidate) {
            return Ok(true);
        }
    }
    Ok(false)
}
