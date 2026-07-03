//! Tuning values for the logic organ: search strategy, occurs-check policy,
//! resource limits, and the aggregate [`LogicConfig`].
//!
//! See the [`README`](https://docs.rs/sim-runtime) for how the logic organ
//! layers concrete resolution over the kernel `Shape` contracts.

use sim_kernel::Symbol;

/// Order in which the resolver explores pending goal states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchStrategy {
    /// Depth-first search: explore the most recent state first (the default).
    #[default]
    Dfs,
    /// Breadth-first search: explore the oldest pending state first.
    Bfs,
    /// Fair search: round-robin over pending states to avoid starvation.
    Fair,
}

impl SearchStrategy {
    /// Parses a strategy from its surface symbol (`dfs`, `bfs`, or `fair`).
    ///
    /// Returns `None` for any other symbol name.
    pub fn from_symbol(symbol: &Symbol) -> Option<Self> {
        match symbol.name.as_ref() {
            "dfs" => Some(Self::Dfs),
            "bfs" => Some(Self::Bfs),
            "fair" => Some(Self::Fair),
            _ => None,
        }
    }

    /// Returns the surface symbol for this strategy (the inverse of
    /// [`SearchStrategy::from_symbol`]).
    pub fn as_symbol(self) -> Symbol {
        Symbol::new(match self {
            Self::Dfs => "dfs",
            Self::Bfs => "bfs",
            Self::Fair => "fair",
        })
    }
}

/// Whether the unifier runs the occurs check when binding a variable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OccursCheck {
    /// Always run the occurs check, rejecting bindings that build cyclic terms
    /// (the safe default).
    #[default]
    Always,
    /// Skip the occurs check, trusting that terms stay acyclic.
    TrustedAcyclic,
}

/// Resource ceilings that bound a single logic query.
///
/// Each limit aborts the query with an error rather than looping forever.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicLimits {
    /// Maximum resolution depth before the query is aborted.
    pub max_depth: usize,
    /// Maximum number of answers to collect, or `None` for unbounded.
    pub max_answers: Option<usize>,
    /// Maximum number of pending goals allowed in a single state.
    pub max_goals: usize,
    /// Maximum number of candidate clauses scanned across the whole query.
    pub max_clause_scan: usize,
}

impl Default for LogicLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_answers: Some(256),
            max_goals: 1024,
            max_clause_scan: 8192,
        }
    }
}

/// Aggregate tuning for the logic organ, threaded through every query.
///
/// Combines [`LogicLimits`], the [`SearchStrategy`], the [`OccursCheck`]
/// policy, and the indexing/tabling switches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicConfig {
    /// Resource ceilings applied to each query.
    pub limits: LogicLimits,
    /// Channel buffer size for streamed answers.
    pub stream_buffer: usize,
    /// Goal-exploration order.
    pub strategy: SearchStrategy,
    /// Occurs-check policy used by the unifier.
    pub occurs_check: OccursCheck,
    /// Whether first-argument clause indexing is enabled.
    pub enable_indexing: bool,
    /// Whether goal tabling (loop detection) is enabled.
    pub enable_tabling: bool,
}

impl Default for LogicConfig {
    fn default() -> Self {
        Self {
            limits: LogicLimits::default(),
            stream_buffer: 64,
            strategy: SearchStrategy::Dfs,
            occurs_check: OccursCheck::Always,
            enable_indexing: true,
            enable_tabling: true,
        }
    }
}
