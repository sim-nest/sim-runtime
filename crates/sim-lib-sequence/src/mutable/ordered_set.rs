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

impl<K> Clone for OrderedSetIter<K> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
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
