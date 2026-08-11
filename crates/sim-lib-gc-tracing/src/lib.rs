#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bounded stop-the-world tracing collection for managed arenas.

use std::{error::Error, fmt};

use sim_lib_mutation::{ArenaError, EdgeId, ManagedId};

mod collector;
mod correctness;
mod finalization;

pub use collector::{collect, collect_with_finalization};
pub use correctness::CorrectnessDimension;
pub use finalization::{FinalizationRecord, FinalizationRegistry};

/// Independently enforced limits for one collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionLimits {
    /// Maximum arena objects admitted.
    pub objects: usize,
    /// Maximum enumerated edges admitted.
    pub edges: usize,
    /// Maximum pending iterative mark stack length.
    pub stack: usize,
    /// Maximum charged root, object, edge, ephemeron, and sweep operations.
    pub work: usize,
    /// Maximum weak and ephemeron entries cleared.
    pub clears: usize,
    /// Maximum finalization records produced and admitted.
    pub finalizers: usize,
}

/// A resource class which refused collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    /// Arena objects.
    Objects,
    /// Enumerated edges.
    Edges,
    /// Pending mark entries.
    Stack,
    /// Total charged operations.
    Work,
    /// Weak and ephemeron clears.
    Clears,
    /// Finalization records.
    Finalizers,
}

/// Inspectable evidence for a collection refused before mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureReceipt {
    /// Snapshot mutation epoch.
    pub mutation_epoch: u64,
    /// Exhausted resource.
    pub kind: LimitKind,
    /// Configured maximum.
    pub limit: usize,
    /// Required amount at refusal.
    pub required: usize,
}

/// Deterministic evidence for a completed collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionReceipt {
    /// Snapshot mutation epoch used to plan the sweep.
    pub mutation_epoch: u64,
    /// Reachable objects in allocation order.
    pub marked: Vec<ManagedId>,
    /// Reclaimed objects in allocation order.
    pub swept: Vec<ManagedId>,
    /// Number of edges enumerated.
    pub edges: usize,
    /// Total charged operations.
    pub work: usize,
    /// Weak edges cleared as `(owner, edge)`.
    pub cleared_weak: Vec<(ManagedId, EdgeId)>,
    /// Ephemerons cleared as `(owner, edge)`.
    pub cleared_ephemerons: Vec<(ManagedId, EdgeId)>,
    /// Finalization records admitted after arena mutation completed.
    pub finalization: Vec<FinalizationRecord>,
}

/// A fail-closed collection error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollectionError {
    /// A budget could not admit the complete read-only plan.
    Limit(FailureReceipt),
    /// The arena rejected a stale edge or atomic sweep.
    Arena(ArenaError),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(r) => write!(
                f,
                "collection {:?} limit {} requires {}",
                r.kind, r.limit, r.required
            ),
            Self::Arena(error) => error.fmt(f),
        }
    }
}
impl Error for CollectionError {}
impl From<ArenaError> for CollectionError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}

#[cfg(test)]
mod tests;
