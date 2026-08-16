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

impl<T: PartialEq> PartialEq for SparseSequence<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self.occupied == other.occupied
            && self.occupied_in(..).eq(other.occupied_in(..))
    }
}

impl<T: Eq> Eq for SparseSequence<T> {}

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
