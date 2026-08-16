//! ECMAScript collection policy composed over shared sequence semantics.

use crate::JavascriptValue;
use sim_lib_sequence::SparseSequence;
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JavascriptMap {
    entries: Vec<(JavascriptValue, JavascriptValue)>,
}
impl JavascriptMap {
    /// Insert or replace without changing insertion position.
    pub fn set(&mut self, key: JavascriptValue, value: JavascriptValue) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|(k, _)| same_value_zero(k, &key))
        {
            e.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }
    /// Lookup a value.
    pub fn get(&self, key: &JavascriptValue) -> Option<&JavascriptValue> {
        self.entries
            .iter()
            .find(|(k, _)| same_value_zero(k, key))
            .map(|e| &e.1)
    }
    /// Delete a key.
    pub fn delete(&mut self, key: &JavascriptValue) -> bool {
        if let Some(i) = self
            .entries
            .iter()
            .position(|(k, _)| same_value_zero(k, key))
        {
            self.entries.remove(i);
            true
        } else {
            false
        }
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
    pub fn entries(&self) -> impl Iterator<Item = (&JavascriptValue, &JavascriptValue)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

/// Insertion-ordered ECMAScript Set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JavascriptSet {
    values: Vec<JavascriptValue>,
}
impl JavascriptSet {
    /// Add a value with SameValueZero uniqueness.
    pub fn add(&mut self, value: JavascriptValue) {
        if !self.has(&value) {
            self.values.push(value);
        }
    }
    /// Membership query.
    pub fn has(&self, value: &JavascriptValue) -> bool {
        self.values.iter().any(|v| same_value_zero(v, value))
    }
    /// Delete a value.
    pub fn delete(&mut self, value: &JavascriptValue) -> bool {
        if let Some(i) = self.values.iter().position(|v| same_value_zero(v, value)) {
            self.values.remove(i);
            true
        } else {
            false
        }
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
        JavascriptIterator::new(self.values.clone())
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
    values: Vec<JavascriptValue>,
    at: usize,
}
impl JavascriptIterator {
    /// Build an iterator over an owned snapshot.
    pub fn new(values: Vec<JavascriptValue>) -> Self {
        Self { values, at: 0 }
    }
    /// Execute the iterator protocol's `next` method.
    pub fn next_result(&mut self) -> JavascriptIteratorResult {
        if let Some(value) = self.values.get(self.at).cloned() {
            self.at += 1;
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
fn same_value_zero(a: &JavascriptValue, b: &JavascriptValue) -> bool {
    match (a, b) {
        (JavascriptValue::Number(a), JavascriptValue::Number(b)) => {
            a == b || (a.is_nan() && b.is_nan())
        }
        _ => a == b,
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
    fn map_set_use_same_value_zero_and_insertion_order() {
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
        let mut s = JavascriptSet::default();
        s.add(JavascriptValue::Number(-0.));
        s.add(JavascriptValue::Number(0.));
        assert_eq!(s.len(), 1);
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
