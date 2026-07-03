use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use sim_kernel::{
    Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Table, Value, object::ClassRef,
};

use crate::standard_mutate_capability;

/// A mutable, symbol-keyed table backing the kernel [`Table`] contract.
///
/// Implements [`Table`] so it behaves as a core table object, but its writing
/// verbs ([`set`](Table::set), [`del`](Table::del), [`clear`](Table::clear))
/// require [`standard_mutate_capability`]; reads are always allowed.
#[sim_citizen_derive::non_citizen(
    reason = "mutable table handle; reconstruct from table entries plus mutation policy",
    kind = "handle",
    descriptor = "core/Table"
)]
pub struct MutableTable {
    entries: RwLock<BTreeMap<Symbol, Value>>,
}

impl MutableTable {
    /// Create a table seeded with `entries` (later duplicates win).
    pub fn new(entries: Vec<(Symbol, Value)>) -> Self {
        Self {
            entries: RwLock::new(entries.into_iter().collect()),
        }
    }
}

impl Object for MutableTable {
    fn display(&self, cx: &mut Cx) -> Result<String> {
        Ok(format!("#<mutation-table {}>", self.len(cx)?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MutableTable {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::CORE_TABLE_CLASS_ID,
            Symbol::qualified("core", "Table"),
        )
    }

    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table_expr(cx)
    }

    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
}

impl Table for MutableTable {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("mutation", "table")
    }

    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.entries
            .read()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .get(&key)
            .cloned()
            .map_or_else(|| cx.factory().nil(), Ok)
    }

    fn set(&self, cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        self.entries
            .write()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .insert(key, value);
        Ok(())
    }

    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .contains_key(&key))
    }

    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        cx.require(&standard_mutate_capability())?;
        self.entries
            .write()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .remove(&key)
            .map_or_else(|| cx.factory().nil(), Ok)
    }

    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .keys()
            .cloned()
            .collect())
    }

    fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect())
    }

    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .len())
    }

    fn clear(&self, cx: &mut Cx) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        self.entries
            .write()
            .map_err(|_| Error::PoisonedLock("mutation table"))?
            .clear();
        Ok(())
    }
}

/// Construct a [`MutableTable`] from `entries` and wrap it as a runtime [`Value`].
pub fn mutable_table(cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
    cx.factory().opaque(Arc::new(MutableTable::new(entries)))
}

/// Borrow the [`MutableTable`] behind `value`, or error if it is not one.
pub fn mutable_table_value(value: &Value) -> Result<&MutableTable> {
    value
        .object()
        .downcast_ref::<MutableTable>()
        .ok_or(Error::TypeMismatch {
            expected: "mutable table",
            found: "non-table",
        })
}
