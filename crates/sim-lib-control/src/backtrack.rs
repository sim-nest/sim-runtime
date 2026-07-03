use sim_kernel::Ref;

/// Result of advancing a [`Backtracker`]: the next alternative, or failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BacktrackStep {
    /// The next untried alternative to commit to.
    Choice(Ref),
    /// No alternatives remain; the search has exhausted this point.
    Failed,
}

/// A linear backtracking choice point over a fixed list of alternatives.
///
/// Models the choose/fail surface of non-deterministic control: each
/// [`Backtracker::choose`] or [`Backtracker::fail`] commits to the next
/// alternative, yielding [`BacktrackStep::Failed`] once they run out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backtracker {
    alternatives: Vec<Ref>,
    index: usize,
}

impl Backtracker {
    /// Builds a backtracker that walks `alternatives` in order.
    pub fn new(alternatives: Vec<Ref>) -> Self {
        Self {
            alternatives,
            index: 0,
        }
    }

    /// Commits to the next alternative, or [`BacktrackStep::Failed`] if none
    /// remain.
    pub fn choose(&mut self) -> BacktrackStep {
        let Some(choice) = self.alternatives.get(self.index).cloned() else {
            return BacktrackStep::Failed;
        };
        self.index += 1;
        BacktrackStep::Choice(choice)
    }

    /// Backtracks the current choice and advances to the next alternative;
    /// equivalent to [`Backtracker::choose`].
    pub fn fail(&mut self) -> BacktrackStep {
        self.choose()
    }
}
