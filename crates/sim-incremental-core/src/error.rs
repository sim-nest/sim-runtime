//! Error records emitted by the incremental query engine.

use std::{error::Error, fmt};

use crate::BudgetKind;

/// An opaque handle that can resume a budget-stopped root query.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContinuationToken(u64);

impl ContinuationToken {
    /// Creates a continuation token from raw bits.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw token bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A typed query verification failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IncrementalError<K> {
    /// No query was registered for the requested key.
    UnknownQuery {
        /// The missing query key.
        key: K,
    },
    /// Query execution attempted to re-enter a key already on the stack.
    Cycle {
        /// The cycle path, including the repeated key at the end.
        path: Vec<K>,
    },
    /// A configured budget was exhausted.
    BudgetExceeded {
        /// The exhausted budget class.
        kind: BudgetKind,
        /// The configured limit.
        limit: usize,
        /// The consumed amount at the failure point.
        consumed: usize,
        /// A token that resumes the owning root query.
        continuation: Option<ContinuationToken>,
    },
    /// Query code requested cancellation.
    Cancelled,
    /// A continuation token was not known to this engine.
    UnknownContinuation {
        /// The rejected token.
        token: ContinuationToken,
    },
}

impl<K> IncrementalError<K> {
    /// Returns the continuation token carried by a budget error, when present.
    #[must_use]
    pub fn continuation(&self) -> Option<ContinuationToken> {
        match self {
            Self::BudgetExceeded { continuation, .. } => *continuation,
            _ => None,
        }
    }
}

impl<K: fmt::Debug> fmt::Display for IncrementalError<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownQuery { key } => write!(f, "unknown query {key:?}"),
            Self::Cycle { path } => write!(f, "incremental query cycle {path:?}"),
            Self::BudgetExceeded {
                kind,
                limit,
                consumed,
                ..
            } => write!(
                f,
                "incremental query budget {kind:?} exhausted at {consumed}/{limit}"
            ),
            Self::Cancelled => f.write_str("incremental query cancelled"),
            Self::UnknownContinuation { token } => {
                write!(f, "unknown continuation token {}", token.get())
            }
        }
    }
}

impl<K: fmt::Debug> Error for IncrementalError<K> {}

/// A graph snapshot restore failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotError<K> {
    /// A snapshot contained the same node key more than once.
    DuplicateNode {
        /// The duplicated key.
        key: K,
    },
}

impl<K: fmt::Debug> fmt::Display for SnapshotError<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode { key } => write!(f, "duplicate snapshot node {key:?}"),
        }
    }
}

impl<K: fmt::Debug> Error for SnapshotError<K> {}
