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

impl<K, V> Clone for OrderedTableIter<K, V> {
    fn clone(&self) -> Self {
        let state = self.state.borrow();
        state
            .active_iterators
            .set(state.active_iterators.get().saturating_add(1));
        drop(state);
        Self {
            state: Rc::clone(&self.state),
            next_slot: self.next_slot,
        }
    }
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
