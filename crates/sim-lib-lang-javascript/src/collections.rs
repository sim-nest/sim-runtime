//! ECMAScript collection policy composed over shared sequence semantics.

use crate::JavascriptValue;
use sim_lib_sequence::{
    OrderedSet, OrderedSetIter, OrderedTable, OrderedTableIter, SparseSequence,
};
use std::collections::BTreeMap;

const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;

/// A unique ECMAScript Symbol identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JavascriptSymbol {
    id: u64,
    description: Option<String>,
}
impl JavascriptSymbol {
    /// Stable identity allocated by a registry.
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Optional descriptive text; it never participates in identity.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// Realm-local allocator and global-symbol registry.
#[derive(Clone, Debug, Default)]
pub struct JavascriptSymbolRegistry {
    next: u64,
    globals: BTreeMap<String, JavascriptSymbol>,
}
impl JavascriptSymbolRegistry {
    /// Allocate a fresh symbol.
    pub fn symbol(&mut self, description: Option<String>) -> JavascriptSymbol {
        let symbol = JavascriptSymbol {
            id: self.next,
            description,
        };
        self.next += 1;
        symbol
    }
    /// Return the stable `Symbol.for` identity for `key`.
    pub fn symbol_for(&mut self, key: impl Into<String>) -> JavascriptSymbol {
        let key = key.into();
        if let Some(symbol) = self.globals.get(&key) {
            return symbol.clone();
        }
        let symbol = self.symbol(Some(key.clone()));
        self.globals.insert(key, symbol.clone());
        symbol
    }
    /// Recover the `Symbol.for` key, if any.
    pub fn key_for(&self, symbol: &JavascriptSymbol) -> Option<&str> {
        self.globals
            .iter()
            .find_map(|(key, value)| (value == symbol).then_some(key.as_str()))
    }
}

/// Failure from a bounded collection method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptCollectionError {
    /// A sparse or explicit index is outside the collection.
    Index,
    /// The caller's explicit work bound was exhausted.
    Limit,
}

/// ECMAScript array with explicit holes distinct from `undefined`.
#[derive(Clone, Debug, PartialEq)]
pub struct JavascriptArray {
    elements: SparseSequence<JavascriptValue>,
}
impl Default for JavascriptArray {
    fn default() -> Self {
        Self::sparse(0)
    }
}
impl JavascriptArray {
    /// Construct a dense array.
    pub fn dense(values: Vec<JavascriptValue>) -> Self {
        let mut elements = SparseSequence::new(MAX_ARRAY_LENGTH);
        for (index, value) in values.into_iter().enumerate() {
            elements
                .set(index, value)
                .expect("a materialized vector has a valid JavaScript array length");
        }
        Self { elements }
    }
    /// Construct with an explicit length and holes.
    pub fn sparse(length: usize) -> Self {
        assert!(
            length <= MAX_ARRAY_LENGTH,
            "invalid JavaScript array length"
        );
        let mut elements = SparseSequence::new(MAX_ARRAY_LENGTH);
        elements.set_len(length).expect("length was checked");
        Self { elements }
    }
    /// ECMAScript length.
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    /// Whether length is zero.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    /// Read an own indexed element; holes remain distinguishable.
    pub fn get(&self, index: usize) -> Option<&JavascriptValue> {
        self.elements.get(index)
    }
    /// Set an index, growing through holes as JavaScript arrays do.
    pub fn set(&mut self, index: usize, value: JavascriptValue) {
        self.elements
            .set(index, value)
            .expect("invalid JavaScript array index");
    }
    /// Set the ECMAScript length, creating holes or deleting truncated values.
    pub fn set_len(&mut self, length: usize) -> Result<(), JavascriptCollectionError> {
        self.elements
            .set_len(length)
            .map_err(|_| JavascriptCollectionError::Index)
    }
    /// Append and return the new length.
    pub fn push(&mut self, value: JavascriptValue) -> usize {
        self.set(self.len(), value);
        self.len()
    }
    /// Remove and return the last element (`undefined` and a hole both return `None` at this policy seam).
    pub fn pop(&mut self) -> Option<JavascriptValue> {
        let index = self.len().checked_sub(1)?;
        let value = self.elements.remove(index);
        self.elements
            .set_len(index)
            .expect("shrinking an array length is valid");
        value
    }
    /// JavaScript array iterator: holes are observed as `undefined`.
    pub fn values(&self) -> JavascriptIterator {
        JavascriptIterator::new(
            (0..self.len())
                .map(|index| {
                    self.get(index)
                        .cloned()
                        .unwrap_or(JavascriptValue::Undefined)
                })
                .collect(),
        )
    }
    /// Bounded `forEach`; callbacks skip holes.
    pub fn for_each(
        &self,
        max_visits: usize,
        mut f: impl FnMut(&JavascriptValue, usize),
    ) -> Result<(), JavascriptCollectionError> {
        for (visit, (index, value)) in self.elements.occupied_in(..).enumerate() {
            if visit >= max_visits {
                return Err(JavascriptCollectionError::Limit);
            }
            f(value, index);
        }
        Ok(())
    }
    /// Bounded `map`; callbacks skip holes and holes are retained.
    pub fn map(
        &self,
        max_visits: usize,
        mut f: impl FnMut(&JavascriptValue, usize) -> JavascriptValue,
    ) -> Result<Self, JavascriptCollectionError> {
        let visits = self.elements.occupied_len();
        if visits > max_visits {
            return Err(JavascriptCollectionError::Limit);
        }
        let mut out = Self::sparse(self.len());
        for (index, value) in self.elements.occupied_in(..) {
            out.set(index, f(value, index));
        }
        Ok(out)
    }
    /// Bounded `filter`; callbacks skip holes and the result is dense.
    pub fn filter(
        &self,
        max_visits: usize,
        mut f: impl FnMut(&JavascriptValue, usize) -> bool,
    ) -> Result<Self, JavascriptCollectionError> {
        let mut out = Self::default();
        let mut visits = 0;
        for (i, value) in self.elements.occupied_in(..) {
            visits += 1;
            if visits > max_visits {
                return Err(JavascriptCollectionError::Limit);
            }
            if f(value, i) {
                out.push(value.clone());
            }
        }
        Ok(out)
    }
}

/// Insertion-ordered ECMAScript Map using SameValueZero-style scalar keys.
#[derive(Debug)]
pub struct JavascriptMap {
    entries: OrderedTable<JavascriptValue, JavascriptValue, SameValueZero>,
}
impl Default for JavascriptMap {
    fn default() -> Self {
        Self {
            entries: OrderedTable::new(SameValueZero),
        }
    }
}
impl Clone for JavascriptMap {
    fn clone(&self) -> Self {
        let copy = Self::default();
        for (key, value) in self.entries.iter() {
            copy.entries.insert(key, value);
        }
        copy
    }
}
impl PartialEq for JavascriptMap {
    fn eq(&self, other: &Self) -> bool {
        self.entries.iter().collect::<Vec<_>>() == other.entries.iter().collect::<Vec<_>>()
    }
}
impl JavascriptMap {
    /// Insert or replace without changing insertion position.
    pub fn set(&mut self, key: JavascriptValue, value: JavascriptValue) {
        self.entries.insert(key, value);
    }
    /// Lookup a value.
    pub fn get(&self, key: &JavascriptValue) -> Option<JavascriptValue> {
        self.entries.get(key)
    }
    /// Delete a key.
    pub fn delete(&mut self, key: &JavascriptValue) -> bool {
        self.entries.remove(key).is_some()
    }
    /// Entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Insertion-ordered entries.
    pub fn entries(&self) -> OrderedTableIter<JavascriptValue, JavascriptValue> {
        self.entries.iter()
    }
}

/// Insertion-ordered ECMAScript Set.
#[derive(Debug)]
pub struct JavascriptSet {
    values: OrderedSet<JavascriptValue, SameValueZero>,
}
impl Default for JavascriptSet {
    fn default() -> Self {
        Self {
            values: OrderedSet::new(SameValueZero),
        }
    }
}
impl Clone for JavascriptSet {
    fn clone(&self) -> Self {
        let copy = Self::default();
        for value in self.values.iter() {
            copy.values.insert(value);
        }
        copy
    }
}
impl PartialEq for JavascriptSet {
    fn eq(&self, other: &Self) -> bool {
        self.values.iter().collect::<Vec<_>>() == other.values.iter().collect::<Vec<_>>()
    }
}
impl JavascriptSet {
    /// Add a value with SameValueZero uniqueness.
    pub fn add(&mut self, value: JavascriptValue) {
        self.values.insert(value);
    }
    /// Membership query.
    pub fn has(&self, value: &JavascriptValue) -> bool {
        self.values.contains(value)
    }
    /// Delete a value.
    pub fn delete(&mut self, value: &JavascriptValue) -> bool {
        self.values.remove(value)
    }
    /// Value count.
    pub fn len(&self) -> usize {
        self.values.len()
    }
    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    /// Insertion-ordered values.
    pub fn values(&self) -> JavascriptIterator {
        JavascriptIterator::live(self.values.iter())
    }
}

/// One iterator result cell.
#[derive(Clone, Debug, PartialEq)]
pub struct JavascriptIteratorResult {
    /// Produced value, absent after completion.
    pub value: Option<JavascriptValue>,
    /// ECMAScript `done` flag.
    pub done: bool,
}
/// Bounded, stateful ECMAScript iterator cell.
#[derive(Clone, Debug)]
pub struct JavascriptIterator {
    source: JavascriptIteratorSource,
}
#[derive(Clone)]
enum JavascriptIteratorSource {
    Snapshot(std::vec::IntoIter<JavascriptValue>),
    Live(OrderedSetIter<JavascriptValue>),
}
impl std::fmt::Debug for JavascriptIteratorSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Snapshot(_) => "Snapshot",
            Self::Live(_) => "Live",
        })
    }
}
impl JavascriptIterator {
    /// Build an iterator over an owned snapshot.
    pub fn new(values: Vec<JavascriptValue>) -> Self {
        Self {
            source: JavascriptIteratorSource::Snapshot(values.into_iter()),
        }
    }
    fn live(values: OrderedSetIter<JavascriptValue>) -> Self {
        Self {
            source: JavascriptIteratorSource::Live(values),
        }
    }
    /// Execute the iterator protocol's `next` method.
    pub fn next_result(&mut self) -> JavascriptIteratorResult {
        let value = match &mut self.source {
            JavascriptIteratorSource::Snapshot(values) => values.next(),
            JavascriptIteratorSource::Live(values) => values.next(),
        };
        if let Some(value) = value {
            JavascriptIteratorResult {
                value: Some(value),
                done: false,
            }
        } else {
            JavascriptIteratorResult {
                value: None,
                done: true,
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct SameValueZero;
impl sim_lib_sequence::KeyEquivalence<JavascriptValue> for SameValueZero {
    fn equivalent(&self, left: &JavascriptValue, right: &JavascriptValue) -> bool {
        match (left, right) {
            (JavascriptValue::Number(a), JavascriptValue::Number(b)) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            _ => left == right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrays_preserve_holes_and_iterators_materialize_undefined() {
        let mut a = JavascriptArray::sparse(2);
        a.set(1, JavascriptValue::Number(2.));
        assert_eq!(a.map(1, |v, _| v.clone()).unwrap().get(0), None);
        let mut it = a.values();
        assert_eq!(it.next_result().value, Some(JavascriptValue::Undefined));
        assert!(!it.next_result().done);
        assert!(it.next_result().done);
    }
    #[test]
    fn array_callbacks_skip_holes_and_length_truncation_deletes_values() {
        let mut array = JavascriptArray::sparse(4);
        array.set(1, JavascriptValue::Number(1.));
        array.set(3, JavascriptValue::Number(3.));
        let mut visited = Vec::new();
        array.for_each(2, |_, index| visited.push(index)).unwrap();
        assert_eq!(visited, vec![1, 3]);
        assert_eq!(array.get(0), None);

        array.set_len(2).unwrap();
        assert_eq!(array.len(), 2);
        assert_eq!(array.get(1), Some(&JavascriptValue::Number(1.)));
        assert_eq!(array.get(3), None);
        array.set_len(4).unwrap();
        assert_eq!(array.get(3), None);
    }
    #[test]
    fn map_nan_keys_match_themselves() {
        let mut m = JavascriptMap::default();
        m.set(
            JavascriptValue::Number(f64::NAN),
            JavascriptValue::Number(1.),
        );
        m.set(
            JavascriptValue::Number(f64::NAN),
            JavascriptValue::Number(2.),
        );
        assert_eq!(m.len(), 1);
        assert_eq!(
            m.get(&JavascriptValue::Number(f64::NAN)),
            Some(JavascriptValue::Number(2.))
        );
    }
    #[test]
    fn set_positive_and_negative_zero_are_the_same_key() {
        let mut s = JavascriptSet::default();
        s.add(JavascriptValue::Number(-0.));
        s.add(JavascriptValue::Number(0.));
        assert_eq!(s.len(), 1);
    }
    #[test]
    fn map_replacement_keeps_position() {
        let mut map = JavascriptMap::default();
        map.set(
            JavascriptValue::String("first".into()),
            JavascriptValue::Number(1.),
        );
        map.set(
            JavascriptValue::String("second".into()),
            JavascriptValue::Number(2.),
        );
        map.set(
            JavascriptValue::String("first".into()),
            JavascriptValue::Number(3.),
        );

        assert_eq!(
            map.entries().collect::<Vec<_>>(),
            vec![
                (
                    JavascriptValue::String("first".into()),
                    JavascriptValue::Number(3.)
                ),
                (
                    JavascriptValue::String("second".into()),
                    JavascriptValue::Number(2.)
                ),
            ]
        );
    }
    #[test]
    fn set_delete_then_reinsert_moves_to_the_end() {
        let mut set = JavascriptSet::default();
        set.add(JavascriptValue::String("first".into()));
        set.add(JavascriptValue::String("second".into()));
        assert!(set.delete(&JavascriptValue::String("first".into())));
        set.add(JavascriptValue::String("first".into()));

        let mut values = set.values();
        assert_eq!(
            values.next_result().value,
            Some(JavascriptValue::String("second".into()))
        );
        assert_eq!(
            values.next_result().value,
            Some(JavascriptValue::String("first".into()))
        );
    }
    #[test]
    fn collection_iterators_visit_entries_added_during_iteration() {
        let mut map = JavascriptMap::default();
        map.set(
            JavascriptValue::String("first".into()),
            JavascriptValue::Number(1.),
        );
        let mut entries = map.entries();
        assert_eq!(
            entries.next().unwrap().0,
            JavascriptValue::String("first".into())
        );
        map.set(
            JavascriptValue::String("second".into()),
            JavascriptValue::Number(2.),
        );
        assert_eq!(
            entries.next().unwrap().0,
            JavascriptValue::String("second".into())
        );

        let mut set = JavascriptSet::default();
        set.add(JavascriptValue::String("first".into()));
        let mut values = set.values();
        assert_eq!(
            values.next_result().value,
            Some(JavascriptValue::String("first".into()))
        );
        set.add(JavascriptValue::String("second".into()));
        assert_eq!(
            values.next_result().value,
            Some(JavascriptValue::String("second".into()))
        );
    }
    #[test]
    fn symbols_have_identity_and_registry_keys() {
        let mut r = JavascriptSymbolRegistry::default();
        assert_ne!(r.symbol(Some("x".into())), r.symbol(Some("x".into())));
        let s = r.symbol_for("x");
        assert_eq!(s, r.symbol_for("x"));
        assert_eq!(r.key_for(&s), Some("x"));
    }
}
