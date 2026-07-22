use sim_kernel::Ref;

/// Outcome of polling an [`AsyncTask`]: pending work or a ready result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncPoll {
    /// The task remains pending and must be polled again.
    Pending,
    /// The task completed, carrying its result [`Ref`].
    Ready(Ref),
}

/// A deterministic async task that resolves after a fixed number of polls.
///
/// The simplest control organ in this crate: it models the poll-to-completion
/// surface of asynchronous behavior without a real executor, so the
/// control-policy contracts can be exercised deterministically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncTask {
    result: Ref,
    pending_polls: usize,
    polls: usize,
}

impl AsyncTask {
    /// Builds a task that reports [`AsyncPoll::Pending`] for `pending_polls`
    /// polls, then yields `result`.
    pub fn ready_after(pending_polls: usize, result: Ref) -> Self {
        Self {
            result,
            pending_polls,
            polls: 0,
        }
    }

    /// Advances the task one step, returning [`AsyncPoll::Pending`] until the
    /// pending count is exhausted and [`AsyncPoll::Ready`] thereafter.
    pub fn poll(&mut self) -> AsyncPoll {
        if self.polls < self.pending_polls {
            self.polls += 1;
            return AsyncPoll::Pending;
        }
        AsyncPoll::Ready(self.result.clone())
    }
}
