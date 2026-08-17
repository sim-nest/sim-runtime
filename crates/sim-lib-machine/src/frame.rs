use sim_lib_control::{AdmissionLimit, WorkLimit};

use crate::ManagedRootSource;
use crate::{CodeCursor, SlotFile, UnitStack, ValueWidthPolicy};

/// One admitted activation, composed entirely from bounded machine state.
pub struct Frame<P, K, R, H>
where
    P: ValueWidthPolicy,
{
    slots: SlotFile<P>,
    operands: UnitStack<P>,
    cursor: CodeCursor,
    continuation: Option<K>,
    roots: R,
    handlers: H,
}

impl<P, K, R, H> Frame<P, K, R, H>
where
    P: ValueWidthPolicy,
{
    /// Composes a frame at a validated code cursor with independently bounded storage.
    pub fn new(
        slot_limit: AdmissionLimit,
        operand_limit: WorkLimit,
        cursor: CodeCursor,
        continuation: Option<K>,
        roots: R,
        handlers: H,
    ) -> Self {
        Self {
            slots: SlotFile::new(slot_limit),
            operands: UnitStack::new(operand_limit),
            cursor,
            continuation,
            roots,
            handlers,
        }
    }

    /// Returns the bounded local-slot file.
    pub fn slots(&self) -> &SlotFile<P> {
        &self.slots
    }

    /// Returns the mutable bounded local-slot file.
    pub fn slots_mut(&mut self) -> &mut SlotFile<P> {
        &mut self.slots
    }

    /// Returns the bounded operand stack.
    pub fn operands(&self) -> &UnitStack<P> {
        &self.operands
    }

    /// Returns the mutable bounded operand stack.
    pub fn operands_mut(&mut self) -> &mut UnitStack<P> {
        &mut self.operands
    }

    /// Returns the current validated instruction cursor.
    pub fn cursor(&self) -> CodeCursor {
        self.cursor
    }

    /// Moves the frame to another validated instruction cursor.
    pub fn set_cursor(&mut self, cursor: CodeCursor) {
        self.cursor = cursor;
    }

    /// Returns the caller-defined continuation state.
    pub fn continuation(&self) -> Option<&K> {
        self.continuation.as_ref()
    }

    /// Returns the caller-defined managed-root state.
    pub fn roots(&self) -> &R {
        &self.roots
    }

    /// Returns the mutable caller-defined managed-root state.
    pub fn roots_mut(&mut self) -> &mut R {
        &mut self.roots
    }

    /// Returns the caller-defined handler state.
    pub fn handlers(&self) -> &H {
        &self.handlers
    }

    /// Returns the mutable caller-defined handler state.
    pub fn handlers_mut(&mut self) -> &mut H {
        &mut self.handlers
    }
}

impl<P, K, R, H> ManagedRootSource for Frame<P, K, R, H>
where
    P: ValueWidthPolicy,
    P::Value: ManagedRootSource,
    K: ManagedRootSource,
    R: ManagedRootSource,
    H: ManagedRootSource,
{
    fn visit_managed_roots(
        &self,
        visit: &mut dyn FnMut(sim_lib_mutation::ManagedId) -> bool,
    ) -> bool {
        let mut complete = true;
        self.slots
            .visit_values(|value| complete = complete && value.visit_managed_roots(visit));
        if !complete {
            return false;
        }
        self.operands
            .visit_values(|value| complete = complete && value.visit_managed_roots(visit));
        if !complete {
            return false;
        }
        if let Some(continuation) = &self.continuation
            && !continuation.visit_managed_roots(visit)
        {
            return false;
        }
        self.roots.visit_managed_roots(visit) && self.handlers.visit_managed_roots(visit)
    }
}

/// Failure to admit another explicit frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStackError {
    /// A push would exceed the caller-declared frame budget.
    DepthExhausted {
        /// Frames present before the refused push.
        depth: usize,
        /// Maximum admitted frame depth.
        limit: usize,
    },
}

/// An explicit activation stack whose depth never consumes the host call stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameStack<F> {
    frames: Vec<F>,
    limit: WorkLimit,
}

impl<F> FrameStack<F> {
    /// Creates an empty frame stack using the control organ's work-limit vocabulary.
    pub fn new(limit: WorkLimit) -> Self {
        Self {
            frames: Vec::new(),
            limit,
        }
    }

    /// Returns the occupied frame depth.
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Admits one frame or returns typed depth exhaustion without recursion.
    pub fn push(&mut self, frame: F) -> Result<(), FrameStackError> {
        if self.frames.len() >= self.limit.0 {
            return Err(FrameStackError::DepthExhausted {
                depth: self.frames.len(),
                limit: self.limit.0,
            });
        }
        self.frames.push(frame);
        Ok(())
    }

    /// Removes the current frame, if any.
    pub fn pop(&mut self) -> Option<F> {
        self.frames.pop()
    }

    /// Returns the current frame, if any.
    pub fn current(&self) -> Option<&F> {
        self.frames.last()
    }

    /// Returns the mutable current frame, if any.
    pub fn current_mut(&mut self) -> Option<&mut F> {
        self.frames.last_mut()
    }

    /// Visits frames in deterministic caller-to-current order.
    pub fn visit_frames(&self, mut visit: impl FnMut(&F)) {
        for frame in &self.frames {
            visit(frame);
        }
    }
}

impl<F: ManagedRootSource> ManagedRootSource for FrameStack<F> {
    fn visit_managed_roots(
        &self,
        visit: &mut dyn FnMut(sim_lib_mutation::ManagedId) -> bool,
    ) -> bool {
        for frame in &self.frames {
            if !frame.visit_managed_roots(visit) {
                return false;
            }
        }
        true
    }
}

/// Malformed value-width evidence in a transfer packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferError {
    /// Value and width sequences have different lengths.
    WidthCountMismatch,
    /// Logical widths must be nonzero.
    ZeroWidth,
}

fn validate_widths<V>(values: &[V], widths: &[usize]) -> Result<(), TransferError> {
    if values.len() != widths.len() {
        return Err(TransferError::WidthCountMismatch);
    }
    if widths.contains(&0) {
        return Err(TransferError::ZeroWidth);
    }
    Ok(())
}

/// Guest-neutral call data: a code reference and width-accounted values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallTransfer<V, C> {
    /// Values entering the target activation.
    pub values: Vec<V>,
    /// Logical storage width corresponding one-for-one with `values`.
    pub widths: Vec<usize>,
    /// Consumer-defined reference to prepared code.
    pub target: C,
}

impl<V, C> CallTransfer<V, C> {
    /// Validates and constructs an explicit call transfer.
    pub fn new(values: Vec<V>, widths: Vec<usize>, target: C) -> Result<Self, TransferError> {
        validate_widths(&values, &widths)?;
        Ok(Self {
            values,
            widths,
            target,
        })
    }
}

/// Guest-neutral return data carrying width-accounted values to a continuation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReturnTransfer<V> {
    /// Values leaving the current activation.
    pub values: Vec<V>,
    /// Logical storage width corresponding one-for-one with `values`.
    pub widths: Vec<usize>,
}

impl<V> ReturnTransfer<V> {
    /// Validates and constructs an explicit return transfer.
    pub fn new(values: Vec<V>, widths: Vec<usize>) -> Result<Self, TransferError> {
        validate_widths(&values, &widths)?;
        Ok(Self { values, widths })
    }
}

/// Explicit control transfer interpreted by a consumer-owned machine driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transfer<V, C> {
    /// Enter prepared code with explicit values and widths.
    Call(CallTransfer<V, C>),
    /// Resume the saved continuation with explicit values and widths.
    Return(ReturnTransfer<V>),
}
