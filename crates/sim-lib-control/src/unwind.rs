/// A language-neutral reason for leaving a dynamic extent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unwind<R, B, C, E> {
    /// Ordinary function or block return.
    Return(R),
    /// Exit a repetition construct.
    Break(B),
    /// Continue a repetition construct.
    Continue(C),
    /// Exceptional completion.
    Exception(E),
    /// Cooperative cancellation.
    Cancelled,
    /// Explicit close of a suspended extent.
    Closed,
}

/// Erased cleanup callback for one dynamic extent.
type Cleanup<U> = Box<dyn FnOnce(&U)>;

/// Cleanup callbacks for nested dynamic extents.
pub struct CleanupStack<U> {
    cleanups: Vec<Cleanup<U>>,
}

impl<U> Default for CleanupStack<U> {
    fn default() -> Self {
        Self {
            cleanups: Vec::new(),
        }
    }
}

impl<U> CleanupStack<U> {
    /// Creates an empty cleanup stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a cleanup for the current nested extent.
    pub fn push(&mut self, cleanup: impl FnOnce(&U) + 'static) {
        self.cleanups.push(Box::new(cleanup));
    }

    /// Runs every cleanup in reverse nesting order for `reason`.
    pub fn unwind(mut self, reason: U) -> U {
        while let Some(cleanup) = self.cleanups.pop() {
            cleanup(&reason);
        }
        reason
    }
}
