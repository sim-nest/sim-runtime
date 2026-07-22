use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use sim_kernel::{Cx, Error, Expr, Object, ObjectCompat, Result, Value};

use crate::{RuntimeKey, RuntimeKeyPolicy, standard_mutate_capability};

/// A mutable table keyed by arbitrary runtime values.
///
/// Unlike [`MutableTable`](crate::MutableTable), this table does not implement
/// the kernel symbol-keyed table contract. It is the shared substrate for guest
/// language hash/map/table values whose keys can be booleans, numbers, strings,
/// symbols, or object identities. Writes require
/// [`standard_mutate_capability`]; reads are always allowed.
#[sim_citizen_derive::non_citizen(
    reason = "mutable runtime-keyed table handle; reconstruct from entries plus key policy",
    kind = "handle",
    descriptor = "core/Expr"
)]
pub struct MutableRuntimeTable<P> {
    policy: P,
    entries: RwLock<BTreeMap<RuntimeKey, Value>>,
}

impl<P: RuntimeKeyPolicy> MutableRuntimeTable<P> {
    /// Creates an empty runtime-keyed table.
    pub fn new(policy: P) -> Self {
        Self {
            policy,
            entries: RwLock::new(BTreeMap::new()),
        }
    }

    /// Creates a runtime-keyed table seeded with `entries`.
    ///
    /// Seeding is construction, not mutation, so it does not require the mutate
    /// capability. Later duplicate keys replace earlier entries.
    pub fn with_entries(cx: &mut Cx, policy: P, entries: Vec<(Value, Value)>) -> Result<Self> {
        let table = Self::new(policy);
        let mut keyed = BTreeMap::new();
        for (key, value) in entries {
            keyed.insert(table.key_for_write(cx, &key)?, value);
        }
        *table.write_entries()? = keyed;
        Ok(table)
    }

    /// Returns the table's key policy.
    pub fn policy(&self) -> &P {
        &self.policy
    }

    /// Reads a value using the configured key policy.
    pub fn get(&self, cx: &mut Cx, key: &Value) -> Result<Option<Value>> {
        let Some(key) = self.policy.key_for(cx, key)? else {
            return Ok(None);
        };
        self.get_runtime_key(&key)
    }

    /// Reads a value using an already-derived runtime key.
    pub fn get_runtime_key(&self, key: &RuntimeKey) -> Result<Option<Value>> {
        Ok(self.read_entries()?.get(key).cloned())
    }

    /// Writes a value using the configured key policy.
    pub fn set(&self, cx: &mut Cx, key: Value, value: Value) -> Result<()> {
        let key = self.key_for_write(cx, &key)?;
        self.set_runtime_key(cx, key, value)
    }

    /// Writes a value using an already-derived runtime key.
    pub fn set_runtime_key(&self, cx: &mut Cx, key: RuntimeKey, value: Value) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        self.write_entries()?.insert(key, value);
        Ok(())
    }

    /// Deletes a value using the configured key policy.
    pub fn del(&self, cx: &mut Cx, key: &Value) -> Result<Option<Value>> {
        let Some(key) = self.policy.key_for(cx, key)? else {
            return Ok(None);
        };
        self.del_runtime_key(cx, &key)
    }

    /// Deletes a value using an already-derived runtime key.
    pub fn del_runtime_key(&self, cx: &mut Cx, key: &RuntimeKey) -> Result<Option<Value>> {
        cx.require(&standard_mutate_capability())?;
        Ok(self.write_entries()?.remove(key))
    }

    /// Returns entries in deterministic key order.
    pub fn entries_in_key_order(&self) -> Result<Vec<(RuntimeKey, Value)>> {
        Ok(self
            .read_entries()?
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect())
    }

    /// Returns the number of entries.
    pub fn len(&self) -> Result<usize> {
        Ok(self.read_entries()?.len())
    }

    /// Returns whether the table is empty.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Clears the table after checking mutation authority.
    pub fn clear(&self, cx: &mut Cx) -> Result<()> {
        cx.require(&standard_mutate_capability())?;
        self.write_entries()?.clear();
        Ok(())
    }

    fn key_for_write(&self, cx: &mut Cx, key: &Value) -> Result<RuntimeKey> {
        self.policy
            .key_for(cx, key)?
            .ok_or_else(|| Error::Eval("runtime table key is not allowed by policy".to_owned()))
    }

    fn read_entries(&self) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<RuntimeKey, Value>>> {
        self.entries
            .read()
            .map_err(|_| Error::PoisonedLock("runtime-keyed mutation table"))
    }

    fn write_entries(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<RuntimeKey, Value>>> {
        self.entries
            .write()
            .map_err(|_| Error::PoisonedLock("runtime-keyed mutation table"))
    }
}

impl<P: RuntimeKeyPolicy + 'static> Object for MutableRuntimeTable<P> {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<runtime-mutation-table {}>", self.len()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<P: RuntimeKeyPolicy + 'static> ObjectCompat for MutableRuntimeTable<P> {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Map(
            self.entries_in_key_order()?
                .into_iter()
                .map(|(key, value)| Ok((key.as_expr(), value.object().as_expr(cx)?)))
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(!self.is_empty()?)
    }
}

/// Constructs a [`MutableRuntimeTable`] and wraps it as a runtime [`Value`].
pub fn mutable_runtime_table<P>(
    cx: &mut Cx,
    policy: P,
    entries: Vec<(Value, Value)>,
) -> Result<Value>
where
    P: RuntimeKeyPolicy + 'static,
{
    let table = MutableRuntimeTable::with_entries(cx, policy, entries)?;
    cx.factory().opaque(Arc::new(table))
}

/// Borrows a [`MutableRuntimeTable`] with policy `P` from `value`.
pub fn mutable_runtime_table_value<P>(value: &Value) -> Result<&MutableRuntimeTable<P>>
where
    P: RuntimeKeyPolicy + 'static,
{
    value
        .object()
        .downcast_ref::<MutableRuntimeTable<P>>()
        .ok_or(Error::TypeMismatch {
            expected: "runtime-keyed mutation table",
            found: "non-table",
        })
}
