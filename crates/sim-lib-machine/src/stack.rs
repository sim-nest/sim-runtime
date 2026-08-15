use std::marker::PhantomData;

use sim_lib_control::WorkLimit;

use crate::ValueWidthPolicy;

/// Exact failure evidence from a unit-accounted operand stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackError {
    /// A push would exceed the configured logical depth.
    Overflow {
        /// Occupied depth before the push.
        depth: usize,
        /// Logical width required by the pushed value.
        width: usize,
        /// Maximum logical depth.
        limit: usize,
    },
    /// A pop was requested from an empty stack.
    Underflow {
        /// Occupied depth at the failed operation.
        depth: usize,
    },
    /// A width policy violated its contract by returning zero.
    ZeroWidth {
        /// Occupied depth at which the invalid value was presented.
        depth: usize,
    },
}

/// A bounded LIFO stack measured in policy-defined logical units.
pub struct UnitStack<P: ValueWidthPolicy> {
    values: Vec<P::Value>,
    depth: usize,
    limit: WorkLimit,
    _policy: PhantomData<P>,
}

impl<P: ValueWidthPolicy> UnitStack<P> {
    /// Creates an empty stack using the control organ's work-limit vocabulary.
    pub fn new(limit: WorkLimit) -> Self {
        Self {
            values: Vec::new(),
            depth: 0,
            limit,
            _policy: PhantomData,
        }
    }

    /// Returns the occupied logical depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns whether the stack contains no values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the top value without removing it.
    pub fn top(&self) -> Result<&P::Value, StackError> {
        self.values
            .last()
            .ok_or(StackError::Underflow { depth: self.depth })
    }

    /// Pushes a value if its complete logical width fits.
    pub fn push(&mut self, value: P::Value) -> Result<(), StackError> {
        let width = P::width(&value);
        if width == 0 {
            return Err(StackError::ZeroWidth { depth: self.depth });
        }
        let next = self
            .depth
            .checked_add(width)
            .filter(|next| *next <= self.limit.0)
            .ok_or(StackError::Overflow {
                depth: self.depth,
                width,
                limit: self.limit.0,
            })?;
        self.values.push(value);
        self.depth = next;
        Ok(())
    }

    /// Pops the top value and releases all logical units it occupied.
    pub fn pop(&mut self) -> Result<P::Value, StackError> {
        let value = self
            .values
            .pop()
            .ok_or(StackError::Underflow { depth: self.depth })?;
        self.depth -= P::width(&value);
        Ok(value)
    }

    /// Releases every value in deterministic LIFO order.
    pub fn clear(&mut self) {
        while self.pop().is_ok() {}
    }
}
