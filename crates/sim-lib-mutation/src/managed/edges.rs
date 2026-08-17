/// Caller-owned role evidence kept separate from managed graph identity.
///
/// Changing a role cannot allocate, remove, renumber, or reorder an edge. The
/// role type is intentionally generic so guests may use open role vocabularies
/// without introducing a global enum in the managed-graph substrate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRole<R> {
    role: R,
}
impl<R> ManagedRole<R> {
    /// Wraps caller-owned role evidence.
    pub const fn new(role: R) -> Self {
        Self { role }
    }

    /// Borrows the current role.
    pub const fn role(&self) -> &R {
        &self.role
    }

    /// Replaces the role and returns the previous evidence.
    pub fn replace_role(&mut self, role: R) -> R {
        std::mem::replace(&mut self.role, role)
    }
}

/// A failed checked mutation of a strong edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrongEdgeMutationError {
    /// Allocating a stable identity for a new edge failed.
    Allocation(EdgeAllocationError),
    /// The requested identity is not a live strong edge of this node.
    UnknownEdge(EdgeId),
    /// The identity is live, but has different collection semantics.
    WrongKind {
        /// Live edge identity supplied by the caller.
        edge: EdgeId,
        /// Actual immutable edge kind.
        actual: EdgeKind,
    },
    /// The edge exists, but no longer names the caller's expected target.
    TargetChanged {
        /// Target supplied by the caller as the mutation precondition.
        expected: ManagedId,
        /// Current target, which was left unchanged.
        actual: ManagedId,
    },
}

/// A failed checked mutation of a weak edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakEdgeMutationError {
    /// Allocating a stable identity for a new edge failed.
    Allocation(EdgeAllocationError),
    /// The requested identity is not a live weak edge of this node.
    UnknownEdge(EdgeId),
    /// The identity is live, but has different collection semantics.
    WrongKind {
        /// Live edge identity supplied by the caller.
        edge: EdgeId,
        /// Actual immutable edge kind.
        actual: EdgeKind,
    },
    /// The edge exists, but no longer names the caller's expected target.
    TargetChanged {
        /// Target supplied by the caller as the mutation precondition.
        expected: ManagedId,
        /// Current target, which was left unchanged.
        actual: ManagedId,
    },
}

/// A failed checked mutation of an ephemeron entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EphemeronMutationError {
    /// Allocating a stable identity for a new entry failed.
    Allocation(EdgeAllocationError),
    /// The requested identity is not a live ephemeron entry of this node.
    UnknownEdge(EdgeId),
    /// The identity is live, but has different collection semantics.
    WrongKind {
        /// Live edge identity supplied by the caller.
        edge: EdgeId,
        /// Actual immutable edge kind.
        actual: EdgeKind,
    },
    /// The entry exists, but no longer contains the caller's expected pair.
    EntryChanged {
        /// Key supplied by the caller as the mutation precondition.
        expected_key: ManagedId,
        /// Value supplied by the caller as the mutation precondition.
        expected_value: ManagedId,
        /// Current key, which was left unchanged.
        actual_key: ManagedId,
        /// Current value, which was left unchanged.
        actual_value: ManagedId,
    },
}

impl fmt::Display for EphemeronMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation(error) => error.fmt(f),
            Self::UnknownEdge(edge) => write!(f, "unknown ephemeron edge {}", edge.0),
            Self::WrongKind { edge, actual } => {
                write!(f, "edge {} is {actual:?}, not Ephemeron", edge.0)
            }
            Self::EntryChanged {
                expected_key,
                expected_value,
                actual_key,
                actual_value,
            } => write!(
                f,
                "ephemeron entry changed from ({}, {}) to ({}, {})",
                expected_key.allocation_ordinal(),
                expected_value.allocation_ordinal(),
                actual_key.allocation_ordinal(),
                actual_value.allocation_ordinal()
            ),
        }
    }
}

impl Error for EphemeronMutationError {}

impl From<EdgeAllocationError> for EphemeronMutationError {
    fn from(error: EdgeAllocationError) -> Self {
        Self::Allocation(error)
    }
}

impl fmt::Display for WeakEdgeMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation(error) => error.fmt(f),
            Self::UnknownEdge(edge) => write!(f, "unknown weak edge {}", edge.0),
            Self::WrongKind { edge, actual } => {
                write!(f, "edge {} is {actual:?}, not Weak", edge.0)
            }
            Self::TargetChanged { expected, actual } => write!(
                f,
                "weak edge target changed from allocation {} to allocation {}",
                expected.allocation_ordinal(),
                actual.allocation_ordinal()
            ),
        }
    }
}

impl Error for WeakEdgeMutationError {}

impl From<EdgeAllocationError> for WeakEdgeMutationError {
    fn from(error: EdgeAllocationError) -> Self {
        Self::Allocation(error)
    }
}

impl fmt::Display for StrongEdgeMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation(error) => error.fmt(f),
            Self::UnknownEdge(edge) => write!(f, "unknown strong edge {}", edge.0),
            Self::WrongKind { edge, actual } => {
                write!(f, "edge {} is {actual:?}, not Strong", edge.0)
            }
            Self::TargetChanged { expected, actual } => write!(
                f,
                "strong edge target changed from allocation {} to allocation {}",
                expected.allocation_ordinal(),
                actual.allocation_ordinal()
            ),
        }
    }
}

impl Error for StrongEdgeMutationError {}

impl From<EdgeAllocationError> for StrongEdgeMutationError {
    fn from(error: EdgeAllocationError) -> Self {
        Self::Allocation(error)
    }
}
