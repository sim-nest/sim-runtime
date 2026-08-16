//! Neutral mutable storage for sparse indexed sequences and ordered keyed collections.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Bound, RangeBounds};
use std::rc::Rc;

const CHUNK_LEN: usize = 64;

/// Policy used to decide whether two table or set keys are equivalent.
pub trait KeyEquivalence<K> {
    /// Return whether `left` and `right` identify the same logical key.
    fn equivalent(&self, left: &K, right: &K) -> bool;
}

impl<K, F> KeyEquivalence<K> for F
where
    F: Fn(&K, &K) -> bool,
{
    fn equivalent(&self, left: &K, right: &K) -> bool {
        self(left, right)
    }
}

#[derive(Clone, Debug)]
struct OrderedEntry<K, V> {
    key: K,
    value: Option<V>,
}

#[derive(Debug)]
struct OrderedState<K, V> {
    entries: Vec<OrderedEntry<K, V>>,
    live_len: usize,
    active_iterators: Cell<usize>,
}

/// Result of an explicit ordered-storage compaction request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionResult {
    /// Live entries were compacted and this many tombstones were removed.
    Compacted(usize),
    /// There were no tombstones to remove.
    NotNeeded,
    /// An active iterator requires entry positions to remain stable.
    ActiveIterator,
    /// The caller's work budget is smaller than the current slot count.
    BudgetExceeded {
        /// Number of slots that compaction would inspect at most once.
        required: usize,
    },
}

/// Mutable insertion-ordered table with caller-defined key equivalence.
///
/// Replacement retains an entry's logical position. Deletion leaves a
/// tombstone, and reinserting an equivalent key appends a new entry. Iterators
/// are live rather than snapshots: they skip entries deleted before visitation
/// and observe entries appended before iteration ends.
#[derive(Debug)]
pub struct OrderedTable<K, V, E> {
    state: Rc<RefCell<OrderedState<K, V>>>,
    equivalence: E,
}

impl<K, V, E> OrderedTable<K, V, E>
where
    E: KeyEquivalence<K>,
{
    /// Construct an empty table using `equivalence` for all key lookup.
    pub fn new(equivalence: E) -> Self {
        Self {
            state: Rc::new(RefCell::new(OrderedState {
                entries: Vec::new(),
                live_len: 0,
                active_iterators: Cell::new(0),
            })),
            equivalence,
        }
    }

    /// Return the number of live entries.
    pub fn len(&self) -> usize {
        self.state.borrow().live_len
    }

    /// Return whether the table contains no live entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return a clone of the value for the equivalent key, if present.
    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        let state = self.state.borrow();
        state
            .entries
            .iter()
            .find(|entry| entry.value.is_some() && self.equivalence.equivalent(&entry.key, key))
            .and_then(|entry| entry.value.clone())
    }

    /// Insert a key/value pair, returning the replaced value when present.
    ///
    /// An equivalent live key is replaced in place. A key equivalent only to a
    /// tombstone is a new insertion and therefore appears at the end.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        let mut state = self.state.borrow_mut();
        if let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.value.is_some() && self.equivalence.equivalent(&entry.key, &key))
        {
            return entry.value.replace(value);
        }
        state.entries.push(OrderedEntry {
            key,
            value: Some(value),
        });
        state.live_len += 1;
        None
    }

    /// Delete an equivalent key, returning its value when present.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut state = self.state.borrow_mut();
        let removed = state
            .entries
            .iter_mut()
            .find(|entry| entry.value.is_some() && self.equivalence.equivalent(&entry.key, key))?
            .value
            .take();
        state.live_len -= 1;
        removed
    }

    /// Create a live insertion-order iterator.
    pub fn iter(&self) -> OrderedTableIter<K, V> {
        let state = self.state.borrow();
        state
            .active_iterators
            .set(state.active_iterators.get().saturating_add(1));
        drop(state);
        OrderedTableIter {
            state: Rc::clone(&self.state),
            next_slot: 0,
        }
    }

    /// Compact tombstones when doing so is position-safe and within `max_work`.
    ///
    /// Work is bounded by the current slot count. The operation is all-or-none:
    /// it does not begin unless that count fits the supplied budget, and it
    /// never runs while an iterator holds a position in this table.
    pub fn compact(&self, max_work: usize) -> CompactionResult {
        let mut state = self.state.borrow_mut();
        if state.active_iterators.get() != 0 {
            return CompactionResult::ActiveIterator;
        }
        let required = state.entries.len();
        if required == state.live_len {
            return CompactionResult::NotNeeded;
        }
        if required > max_work {
            return CompactionResult::BudgetExceeded { required };
        }
        let removed = required - state.live_len;
        state.entries.retain(|entry| entry.value.is_some());
        CompactionResult::Compacted(removed)
    }

    #[cfg(test)]
    fn slot_len(&self) -> usize {
        self.state.borrow().entries.len()
    }
}

/// Live iterator over cloned insertion-ordered table entries.
pub struct OrderedTableIter<K, V> {
    state: Rc<RefCell<OrderedState<K, V>>>,
    next_slot: usize,
}

impl<K, V> Iterator for OrderedTableIter<K, V>
where
    K: Clone,
    V: Clone,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        let state = self.state.borrow();
        while self.next_slot < state.entries.len() {
            let slot = self.next_slot;
            self.next_slot += 1;
            let entry = &state.entries[slot];
            if let Some(value) = &entry.value {
                return Some((entry.key.clone(), value.clone()));
            }
        }
        None
    }
}

impl<K, V> Drop for OrderedTableIter<K, V> {
    fn drop(&mut self) {
        let state = self.state.borrow();
        state
            .active_iterators
            .set(state.active_iterators.get().saturating_sub(1));
    }
}

/// Mutable insertion-ordered set with caller-defined key equivalence.
#[derive(Debug)]
pub struct OrderedSet<K, E> {
    table: OrderedTable<K, (), E>,
}

impl<K, E> OrderedSet<K, E>
where
    E: KeyEquivalence<K>,
{
    /// Construct an empty set using `equivalence` for membership.
    pub fn new(equivalence: E) -> Self {
        Self {
            table: OrderedTable::new(equivalence),
        }
    }

    /// Return the number of members.
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// Return whether the set contains no members.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Return whether an equivalent member is present.
    pub fn contains(&self, key: &K) -> bool {
        self.table.get(key).is_some()
    }

    /// Insert `key`, returning whether it was newly added.
    pub fn insert(&self, key: K) -> bool {
        self.table.insert(key, ()).is_none()
    }

    /// Remove an equivalent member, returning whether it was present.
    pub fn remove(&self, key: &K) -> bool {
        self.table.remove(key).is_some()
    }

    /// Create a live insertion-order iterator.
    pub fn iter(&self) -> OrderedSetIter<K> {
        OrderedSetIter {
            inner: self.table.iter(),
        }
    }

    /// Compact tombstones under the table's position and work rules.
    pub fn compact(&self, max_work: usize) -> CompactionResult {
        self.table.compact(max_work)
    }
}

/// Live iterator over cloned insertion-ordered set members.
pub struct OrderedSetIter<K> {
    inner: OrderedTableIter<K, ()>,
}

impl<K> Iterator for OrderedSetIter<K>
where
    K: Clone,
{
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(key, ())| key)
    }
}

/// A failed sparse-sequence growth or length mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SparseSequenceError {
    /// The requested logical length exceeds the configured limit.
    LengthLimit {
        /// The requested logical length.
        requested: usize,
        /// The greatest permitted logical length.
        limit: usize,
    },
    /// An index could not be converted into the required logical length.
    IndexOverflow {
        /// The index that could not be represented as `index + 1`.
        index: usize,
    },
}

impl fmt::Display for SparseSequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthLimit { requested, limit } => {
                write!(
                    formatter,
                    "sparse sequence length {requested} exceeds limit {limit}"
                )
            }
            Self::IndexOverflow { index } => {
                write!(
                    formatter,
                    "sparse sequence index {index} cannot grow the length"
                )
            }
        }
    }
}

impl std::error::Error for SparseSequenceError {}

/// Mutable sparse indexed storage with stable holes and bounded allocation.
///
/// Logical length is stored separately from values. Writing a distant index
/// allocates only its fixed-size chunk; intervening indices remain holes.
/// `max_len` is an explicit work limit applied to every operation that can grow
/// the logical sequence.
#[derive(Clone, Debug)]
pub struct SparseSequence<T> {
    chunks: BTreeMap<usize, Box<[Option<T>; CHUNK_LEN]>>,
    len: usize,
    occupied: usize,
    max_len: usize,
    revision: u64,
}

impl<T> SparseSequence<T> {
    /// Construct an empty store whose logical length may not exceed `max_len`.
    pub fn new(max_len: usize) -> Self {
        Self {
            chunks: BTreeMap::new(),
            len: 0,
            occupied: 0,
            max_len,
            revision: 0,
        }
    }

    /// Return the logical length, including holes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return whether the logical sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Return the configured logical-length limit.
    pub fn max_len(&self) -> usize {
        self.max_len
    }

    /// Return the number of occupied positions.
    pub fn occupied_len(&self) -> usize {
        self.occupied
    }

    /// Return the mutation revision.
    ///
    /// It advances for every successful operation that changes length or an
    /// occupied position, using wrapping arithmetic so mutation never fails
    /// merely because the diagnostic counter reached its integer limit.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Return the value at `index`, or `None` for a hole or out-of-range index.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let (chunk, offset) = split_index(index);
        self.chunks.get(&chunk)?.get(offset)?.as_ref()
    }

    /// Return whether `index` is an occupied in-range position.
    pub fn contains_index(&self, index: usize) -> bool {
        self.get(index).is_some()
    }

    /// Set `index`, growing the logical length with holes when necessary.
    ///
    /// Returns the previously stored value, if the position was occupied.
    pub fn set(&mut self, index: usize, value: T) -> Result<Option<T>, SparseSequenceError> {
        let required_len = index
            .checked_add(1)
            .ok_or(SparseSequenceError::IndexOverflow { index })?;
        self.check_len(required_len)?;

        let (chunk_index, offset) = split_index(index);
        let chunk = self
            .chunks
            .entry(chunk_index)
            .or_insert_with(|| Box::new(std::array::from_fn(|_| None)));
        let previous = chunk[offset].replace(value);
        if previous.is_none() {
            self.occupied += 1;
        }
        self.len = self.len.max(required_len);
        self.bump_revision();
        Ok(previous)
    }

    /// Remove and return the value at `index`, leaving a stable hole.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }
        let (chunk_index, offset) = split_index(index);
        let chunk = self.chunks.get_mut(&chunk_index)?;
        let removed = chunk[offset].take()?;
        self.occupied -= 1;
        if chunk.iter().all(Option::is_none) {
            self.chunks.remove(&chunk_index);
        }
        self.bump_revision();
        Some(removed)
    }

    /// Set the logical length, creating holes on growth and dropping values on
    /// truncation.
    pub fn set_len(&mut self, new_len: usize) -> Result<(), SparseSequenceError> {
        self.check_len(new_len)?;
        if new_len == self.len {
            return Ok(());
        }
        if new_len < self.len {
            self.truncate_values(new_len);
        }
        self.len = new_len;
        self.bump_revision();
        Ok(())
    }

    /// Traverse occupied `(index, value)` pairs within a bounded index range.
    ///
    /// The range is intersected with the logical sequence, and holes do not
    /// produce iterator items or work proportional to the logical length.
    pub fn occupied_in<R>(&self, range: R) -> impl Iterator<Item = (usize, &T)>
    where
        R: RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            Bound::Included(index) => *index,
            Bound::Excluded(index) => index.saturating_add(1),
            Bound::Unbounded => 0,
        }
        .min(self.len);
        let end = match range.end_bound() {
            Bound::Included(index) => index.saturating_add(1),
            Bound::Excluded(index) => *index,
            Bound::Unbounded => self.len,
        }
        .min(self.len)
        .max(start);
        let first_chunk = start / CHUNK_LEN;
        let end_chunk = end / CHUNK_LEN + usize::from(end % CHUNK_LEN != 0);

        self.chunks
            .range(first_chunk..end_chunk)
            .flat_map(move |(chunk_index, chunk)| {
                chunk.iter().enumerate().filter_map(move |(offset, value)| {
                    let index = chunk_index * CHUNK_LEN + offset;
                    (start..end)
                        .contains(&index)
                        .then(|| value.as_ref().map(|value| (index, value)))
                        .flatten()
                })
            })
    }

    fn check_len(&self, requested: usize) -> Result<(), SparseSequenceError> {
        if requested > self.max_len {
            return Err(SparseSequenceError::LengthLimit {
                requested,
                limit: self.max_len,
            });
        }
        Ok(())
    }

    fn truncate_values(&mut self, new_len: usize) {
        let first_removed_chunk = new_len / CHUNK_LEN;
        let first_removed_offset = new_len % CHUNK_LEN;

        if first_removed_offset != 0
            && let Some(chunk) = self.chunks.get_mut(&first_removed_chunk)
        {
            for slot in &mut chunk[first_removed_offset..] {
                if slot.take().is_some() {
                    self.occupied -= 1;
                }
            }
            if chunk.iter().all(Option::is_none) {
                self.chunks.remove(&first_removed_chunk);
            }
        }

        let remove_from = first_removed_chunk + usize::from(first_removed_offset != 0);
        let removed = self.chunks.split_off(&remove_from);
        self.occupied -= removed
            .values()
            .map(|chunk| chunk.iter().filter(|slot| slot.is_some()).count())
            .sum::<usize>();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

fn split_index(index: usize) -> (usize, usize) {
    (index / CHUNK_LEN, index % CHUNK_LEN)
}

#[cfg(test)]
mod tests {
    use super::{CompactionResult, OrderedSet, OrderedTable, SparseSequence, SparseSequenceError};

    fn exact(left: &&str, right: &&str) -> bool {
        left == right
    }

    #[test]
    fn delete_and_reinsert_moves_key_to_end() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("second", 2);
        assert_eq!(table.insert("first", 3), Some(1));
        assert_eq!(table.remove(&"first"), Some(3));
        table.insert("first", 4);

        assert_eq!(
            table.iter().collect::<Vec<_>>(),
            vec![("second", 2), ("first", 4)]
        );
    }

    #[test]
    fn iterator_observes_delete_and_append_without_losing_position() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("deleted", 2);
        table.insert("third", 3);
        let mut iterator = table.iter();

        assert_eq!(iterator.next(), Some(("first", 1)));
        table.remove(&"deleted");
        table.insert("appended", 4);
        assert_eq!(
            iterator.collect::<Vec<_>>(),
            vec![("third", 3), ("appended", 4)]
        );
    }

    #[test]
    fn compaction_is_bounded_and_blocked_by_active_iterator() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("deleted", 2);
        table.insert("third", 3);
        table.remove(&"deleted");
        let iterator = table.iter();

        assert_eq!(table.compact(usize::MAX), CompactionResult::ActiveIterator);
        assert_eq!(table.slot_len(), 3);
        drop(iterator);
        assert_eq!(
            table.compact(2),
            CompactionResult::BudgetExceeded { required: 3 }
        );
        assert_eq!(table.slot_len(), 3);
        assert_eq!(table.compact(3), CompactionResult::Compacted(1));
        assert_eq!(table.slot_len(), 2);
    }

    #[test]
    fn set_uses_the_supplied_equivalence_policy() {
        let set = OrderedSet::new(|left: &String, right: &String| left.eq_ignore_ascii_case(right));
        assert!(set.insert("Alpha".to_owned()));
        assert!(!set.insert("alpha".to_owned()));
        assert!(set.contains(&"ALPHA".to_owned()));
        assert_eq!(set.iter().collect::<Vec<_>>(), vec!["Alpha".to_owned()]);
    }

    #[test]
    fn distant_write_allocates_only_occupied_storage() {
        let mut sequence = SparseSequence::new(1_000_001);
        sequence.set(1_000_000, "far").unwrap();

        assert_eq!(sequence.len(), 1_000_001);
        assert_eq!(sequence.occupied_len(), 1);
        assert_eq!(sequence.chunks.len(), 1);
        assert_eq!(sequence.get(999_999), None);
        assert_eq!(sequence.get(1_000_000), Some(&"far"));
    }

    #[test]
    fn generated_operations_preserve_sparse_model() {
        let mut sequence = SparseSequence::new(257);
        let mut model = Vec::<Option<u32>>::new();
        let mut state = 0x5eed_u64;

        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let index = (state as usize) % 257;
            match (state >> 32) % 3 {
                0 => {
                    let value = state as u32;
                    sequence.set(index, value).unwrap();
                    model.resize(model.len().max(index + 1), None);
                    model[index] = Some(value);
                }
                1 => {
                    assert_eq!(
                        sequence.remove(index),
                        model.get_mut(index).and_then(Option::take)
                    );
                }
                _ => {
                    let new_len = index;
                    sequence.set_len(new_len).unwrap();
                    model.resize(new_len, None);
                }
            }

            assert_eq!(sequence.len(), model.len());
            assert_eq!(sequence.occupied_len(), model.iter().flatten().count());
            assert_eq!(
                sequence
                    .occupied_in(..)
                    .map(|(index, value)| (index, *value))
                    .collect::<Vec<_>>(),
                model
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| value.map(|value| (index, value)))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn growth_limits_fail_without_mutation() {
        let mut sequence = SparseSequence::new(4);
        sequence.set(3, 7).unwrap();
        let revision = sequence.revision();

        assert_eq!(
            sequence.set(4, 8),
            Err(SparseSequenceError::LengthLimit {
                requested: 5,
                limit: 4
            })
        );
        assert_eq!(
            sequence.set_len(5),
            Err(SparseSequenceError::LengthLimit {
                requested: 5,
                limit: 4
            })
        );
        assert_eq!(sequence.revision(), revision);
        assert_eq!(sequence.get(3), Some(&7));
    }

    #[test]
    fn truncation_drops_values_and_growth_restores_holes() {
        let mut sequence = SparseSequence::new(200);
        sequence.set(2, 'a').unwrap();
        sequence.set(130, 'b').unwrap();
        sequence.set_len(64).unwrap();
        sequence.set_len(131).unwrap();

        assert_eq!(sequence.get(2), Some(&'a'));
        assert_eq!(sequence.get(130), None);
        assert_eq!(sequence.occupied_len(), 1);
        assert_eq!(sequence.remove(2), Some('a'));
        assert_eq!(sequence.len(), 131);
        assert_eq!(sequence.occupied_len(), 0);
    }
}
