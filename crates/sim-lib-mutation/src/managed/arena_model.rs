/// Receives every outgoing managed edge of an object.
pub trait EdgeVisitor {
    /// Visits a retaining edge.
    fn strong(&mut self, edge: EdgeId, target: ManagedId);

    /// Visits a non-retaining edge that may be cleared after tracing.
    fn weak(&mut self, edge: EdgeId, target: ManagedId);

    /// Visits a value retained only when `key` is reachable.
    fn ephemeron(&mut self, edge: EdgeId, key: ManagedId, value: ManagedId);
}
/// An object stored by [`ManagedArena`].
pub trait ManagedObject {
    /// Enumerates all strong, weak, and ephemeron edges exactly once.
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor);

    /// Clears one weak edge if it still points at `expected`.
    ///
    /// Returning `true` means this invocation performed the clear. Repeating
    /// the same request must return `false`, giving collectors at-most-once
    /// weak-clear semantics.
    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool;

    /// Clears one ephemeron entry if it still has the expected key and value.
    /// Repeating a successful request must return `false`.
    fn clear_ephemeron_edge(
        &mut self,
        _edge: EdgeId,
        _expected_key: ManagedId,
        _expected_value: ManagedId,
    ) -> bool {
        false
    }
}

/// The only built-in policy: retain objects until explicit teardown, while
/// refusing allocations beyond a fixed hard cap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardCappedRetainPolicy {
    max_objects: usize,
}

impl HardCappedRetainPolicy {
    /// Creates a retain policy with a non-zero object cap.
    pub fn new(max_objects: usize) -> Result<Self, ArenaError> {
        if max_objects == 0 {
            return Err(ArenaError::InvalidCap);
        }
        Ok(Self { max_objects })
    }

    /// Returns the allocation cap.
    pub const fn max_objects(self) -> usize {
        self.max_objects
    }
}

/// Fail-closed arena operation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArenaError {
    /// A zero-sized arena was requested.
    InvalidCap,
    /// Allocation would exceed the hard object cap.
    CapacityExceeded {
        /// Configured maximum number of live objects.
        cap: usize,
    },
    /// The allocation or root identity space is exhausted.
    IdentityExhausted,
    /// A handle no longer names a live object.
    StaleHandle(ManagedId),
    /// A root registration is unknown or does not match the handle.
    StaleRoot(RootId),
    /// A rooted object cannot be removed.
    ObjectRooted(ManagedId),
    /// Collection was planned against a different graph state.
    MutationEpochChanged {
        /// Epoch used to prepare the operation.
        expected: u64,
        /// Current arena epoch.
        actual: u64,
    },
}

impl fmt::Display for ArenaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCap => f.write_str("managed arena cap must be non-zero"),
            Self::CapacityExceeded { cap } => write!(f, "managed arena hard cap {cap} reached"),
            Self::IdentityExhausted => f.write_str("managed arena identity space exhausted"),
            Self::StaleHandle(id) => write!(f, "stale managed handle {}", id.0),
            Self::StaleRoot(id) => write!(f, "stale managed root {}", id.0),
            Self::ObjectRooted(id) => write!(f, "managed object {} is rooted", id.0),
            Self::MutationEpochChanged { expected, actual } => write!(
                f,
                "managed arena mutation epoch changed from {expected} to {actual}"
            ),
        }
    }
}

impl Error for ArenaError {}

/// An immutable, complete tracing view taken at a safepoint.
pub struct TraceSnapshot<'a, T> {
    roots: Vec<ManagedId>,
    kept_alive: Vec<ManagedId>,
    objects: &'a BTreeMap<ManagedId, T>,
    mutation_epoch: u64,
}

impl<T: ManagedObject> TraceSnapshot<'_, T> {
    /// Returns the arena mutation epoch captured by this snapshot.
    pub const fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }
    /// Enumerates roots in root-registration order.
    pub fn roots(&self) -> impl ExactSizeIterator<Item = ManagedId> + '_ {
        self.roots.iter().copied()
    }

    /// Enumerates successful weak dereferences kept alive for this epoch.
    pub fn kept_alive(&self) -> impl ExactSizeIterator<Item = ManagedId> + '_ {
        self.kept_alive.iter().copied()
    }

    /// Enumerates live objects in allocation order.
    pub fn objects(&self) -> impl ExactSizeIterator<Item = ManagedId> + '_ {
        self.objects.keys().copied()
    }

    /// Visits all edges for a live object.
    pub fn visit_edges(
        &self,
        owner: ManagedId,
        visitor: &mut dyn EdgeVisitor,
    ) -> Result<(), ArenaError> {
        self.objects
            .get(&owner)
            .ok_or(ArenaError::StaleHandle(owner))?
            .trace_edges(visitor);
        Ok(())
    }
}

/// Deterministic evidence for one tracing safepoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafepointReceipt {
    /// Monotonic zero-based safepoint sequence.
    pub sequence: u64,
    /// Roots in root-registration order.
    pub roots: Vec<ManagedId>,
    /// Live objects in allocation order.
    pub objects: Vec<ManagedId>,
}

/// Bounded, allocation-ordered audit evidence projected from one safepoint.
///
/// Labels are caller-owned metadata. They are deliberately absent from tracing
/// and collection contracts, so changing them cannot affect reachability or
/// reclamation policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleProjectionReceipt<L> {
    /// The base tracing receipt this optional evidence describes.
    pub safepoint: SafepointReceipt,
    /// Arena mutation epoch observed by the projection.
    pub mutation_epoch: u64,
    /// Owner identities and labels in managed allocation order.
    pub roles: Vec<(ManagedId, L)>,
}

/// A failed bounded role-evidence projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoleProjectionError {
    /// The requested projection would exceed its explicit row limit.
    Limit {
        /// Maximum admitted role rows.
        limit: usize,
        /// Number of live managed objects requiring rows.
        required: usize,
    },
    /// The arena rejected the safepoint.
    Arena(ArenaError),
}

impl fmt::Display for RoleProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { limit, required } => {
                write!(f, "role projection limit {limit} requires {required}")
            }
            Self::Arena(error) => error.fmt(f),
        }
    }
}

impl Error for RoleProjectionError {}

impl From<ArenaError> for RoleProjectionError {
    fn from(error: ArenaError) -> Self {
        Self::Arena(error)
    }
}

/// Deterministic evidence returned by explicit arena teardown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TeardownReceipt {
    /// Objects removed in allocation order.
    pub objects: Vec<ManagedId>,
    /// Root registrations removed in registration order.
    pub roots: Vec<RootId>,
}

/// Atomic collector mutation evidence.
pub struct CollectionMutationReceipt {
    /// Weak entries cleared as owner and edge identities.
    pub cleared_weak: Vec<(ManagedId, EdgeId)>,
    /// Ephemeron entries cleared as owner and edge identities.
    pub cleared_ephemerons: Vec<(ManagedId, EdgeId)>,
    /// Objects removed in allocation order.
    pub swept: Vec<ManagedId>,
}
