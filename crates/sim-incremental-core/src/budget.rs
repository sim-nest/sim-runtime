//! Budget records for incremental query verification and snapshots.

/// The resource class that exhausted a query run.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BudgetKind {
    /// Query execution work units were exhausted.
    Work,
    /// Dependency observation records were exhausted.
    Observations,
    /// Nested query depth was exhausted.
    Depth,
    /// Output units were exhausted.
    Output,
}

/// Limits applied while verifying queries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryBudgets {
    /// Maximum query executions and explicit work charges.
    pub max_work: usize,
    /// Maximum dependency observations recorded during the run.
    pub max_observations: usize,
    /// Maximum nested query stack depth.
    pub max_depth: usize,
    /// Maximum output units charged by query results or user code.
    pub max_output: usize,
}

impl QueryBudgets {
    /// Returns an unbounded query budget.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_work: usize::MAX,
            max_observations: usize::MAX,
            max_depth: usize::MAX,
            max_output: usize::MAX,
        }
    }

    /// Returns a query budget with explicit limits.
    #[must_use]
    pub const fn new(
        max_work: usize,
        max_observations: usize,
        max_depth: usize,
        max_output: usize,
    ) -> Self {
        Self {
            max_work,
            max_observations,
            max_depth,
            max_output,
        }
    }
}

impl Default for QueryBudgets {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Limits applied when exporting a graph snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotBudgets {
    /// Maximum memo nodes exported into the snapshot.
    pub max_nodes: usize,
    /// Maximum dependency edges exported into the snapshot.
    pub max_edges: usize,
}

impl SnapshotBudgets {
    /// Returns an unbounded snapshot budget.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_nodes: usize::MAX,
            max_edges: usize::MAX,
        }
    }

    /// Returns a snapshot budget with explicit node and edge limits.
    #[must_use]
    pub const fn new(max_nodes: usize, max_edges: usize) -> Self {
        Self {
            max_nodes,
            max_edges,
        }
    }
}

impl Default for SnapshotBudgets {
    fn default() -> Self {
        Self::unlimited()
    }
}
