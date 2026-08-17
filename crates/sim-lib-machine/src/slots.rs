use std::marker::PhantomData;

use sim_lib_control::AdmissionLimit;

use crate::ValueWidthPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    width: usize,
}

/// Exact failure evidence from indexed slot storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// The requested value would extend beyond the slot-file limit.
    Overflow {
        /// First slot requested by the operation.
        slot: usize,
        /// Logical width required by the value.
        width: usize,
        /// Total number of slots admitted for the file.
        limit: usize,
    },
    /// The named slot is not the initialized start of a value.
    Uninitialized {
        /// Slot whose initialized value was requested.
        slot: usize,
    },
    /// A width policy violated its contract by returning zero.
    ZeroWidth {
        /// Slot at which the invalid value was presented.
        slot: usize,
    },
}

/// Bounded indexed storage whose occupancy is measured in policy-defined units.
///
/// A value is loaded only through the first slot in its span. Replacing any
/// unit of an initialized span first releases the entire old value, so partial
/// overwrites are always observable as explicit uninitialization.
pub struct SlotFile<P: ValueWidthPolicy> {
    values: Vec<Option<P::Value>>,
    occupancy: Vec<Option<Span>>,
    _policy: PhantomData<P>,
}

impl<P: ValueWidthPolicy> SlotFile<P> {
    /// Creates an uninitialized slot file using the control organ's admission limit.
    pub fn new(limit: AdmissionLimit) -> Self {
        Self {
            values: (0..limit.0).map(|_| None).collect(),
            occupancy: vec![None; limit.0],
            _policy: PhantomData,
        }
    }

    /// Returns the number of logical slots in the file.
    pub fn limit(&self) -> usize {
        self.occupancy.len()
    }

    /// Returns the initialized value beginning at `slot`.
    pub fn load(&self, slot: usize) -> Result<&P::Value, SlotError> {
        self.values
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(SlotError::Uninitialized { slot })
    }

    /// Stores `value` beginning at `slot`, releasing every overlapped span.
    pub fn store(&mut self, slot: usize, value: P::Value) -> Result<(), SlotError> {
        let width = P::width(&value);
        if width == 0 {
            return Err(SlotError::ZeroWidth { slot });
        }
        let end = slot
            .checked_add(width)
            .filter(|end| *end <= self.limit())
            .ok_or(SlotError::Overflow {
                slot,
                width,
                limit: self.limit(),
            })?;

        let mut overlaps = self.occupancy[slot..end]
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        overlaps.sort_unstable_by_key(|span| span.start);
        overlaps.dedup();
        for span in overlaps {
            self.release_span(span);
        }

        let span = Span { start: slot, width };
        self.values[slot] = Some(value);
        self.occupancy[slot..end].fill(Some(span));
        Ok(())
    }

    /// Releases the initialized span containing `slot` and clears all its units.
    pub fn release(&mut self, slot: usize) -> Result<P::Value, SlotError> {
        let span = self
            .occupancy
            .get(slot)
            .copied()
            .flatten()
            .ok_or(SlotError::Uninitialized { slot })?;
        self.release_span(span)
            .ok_or(SlotError::Uninitialized { slot })
    }

    /// Returns whether `slot` is occupied by any initialized span.
    pub fn is_initialized(&self, slot: usize) -> bool {
        self.occupancy.get(slot).is_some_and(Option::is_some)
    }

    /// Visits initialized values in ascending start-slot order.
    pub fn visit_values(&self, mut visit: impl FnMut(&P::Value)) {
        for value in self.values.iter().flatten() {
            visit(value);
        }
    }

    fn release_span(&mut self, span: Span) -> Option<P::Value> {
        self.occupancy[span.start..span.start + span.width].fill(None);
        self.values[span.start].take()
    }
}
