use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

/// The tracing ABI understood by this arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceContractVersion {
    /// Roots, strong and weak edges, ephemerons, safepoints, clearing, and teardown.
    V1,
}

/// A stable managed-object identity assigned from allocation order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ManagedId(u64);

impl ManagedId {
    /// Returns the zero-based allocation ordinal.
    pub const fn allocation_ordinal(self) -> u64 {
        self.0
    }
}

/// A stable identity for one root registration.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct RootId(u64);

/// An object handle. It does not itself keep the object rooted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ManagedHandle {
    id: ManagedId,
}

impl ManagedHandle {
    /// Returns the managed identity.
    pub const fn id(self) -> ManagedId {
        self.id
    }

    /// Produces a non-rooting weak handle.
    pub const fn downgrade(self) -> WeakHandle {
        WeakHandle { id: self.id }
    }
}

/// A registered root handle. Dropping this value does not mutate the arena;
/// callers explicitly release it so root changes remain transactional.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RootedHandle {
    root: RootId,
    handle: ManagedHandle,
}

impl RootedHandle {
    /// Returns the root registration identity.
    pub const fn root_id(self) -> RootId {
        self.root
    }

    /// Returns the underlying object handle.
    pub const fn handle(self) -> ManagedHandle {
        self.handle
    }
}

/// A non-rooting object handle which may become stale.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct WeakHandle {
    id: ManagedId,
}

impl WeakHandle {
    /// Returns the identity without claiming the object is still live.
    pub const fn id(self) -> ManagedId {
        self.id
    }
}

/// Stable identity of an edge within its owning object.
///
/// Identities are allocated monotonically by [`EdgeAllocator`]. They are local
/// to one managed object, remain ordered by allocation, and are never reused.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct EdgeId(pub u32);

impl EdgeId {
    /// Returns the zero-based allocation ordinal within the owning object.
    pub const fn allocation_ordinal(self) -> u32 {
        self.0
    }
}

/// The collection semantics of a managed edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EdgeKind {
    /// A retaining edge.
    Strong,
    /// A non-retaining edge that may be cleared.
    Weak,
    /// A key/value edge whose value is retained only by a reachable key.
    Ephemeron,
}

/// Hard limits for the outgoing edges owned by one [`ManagedNode`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeLimits {
    total: usize,
    strong: usize,
    weak: usize,
    ephemeron: usize,
}

impl EdgeLimits {
    /// Default per-node hard cap. Per-kind caps share the same ceiling while
    /// the total cap prevents their sum from exceeding it.
    pub const DEFAULT: Self = Self::new(65_536, 65_536, 65_536, 65_536);

    /// Defines the total and per-kind edge caps. Zero is a valid cap.
    pub const fn new(total: usize, strong: usize, weak: usize, ephemeron: usize) -> Self {
        Self {
            total,
            strong,
            weak,
            ephemeron,
        }
    }

    /// Returns the total outgoing-edge cap.
    pub const fn total(self) -> usize {
        self.total
    }

    /// Returns the cap for `kind`.
    pub const fn for_kind(self, kind: EdgeKind) -> usize {
        match kind {
            EdgeKind::Strong => self.strong,
            EdgeKind::Weak => self.weak,
            EdgeKind::Ephemeron => self.ephemeron,
        }
    }
}

impl Default for EdgeLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One allocation-ordered edge in a deterministic node snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeSnapshot {
    /// A retaining edge.
    Strong {
        /// Stable edge identity.
        edge: EdgeId,
        /// Retained target.
        target: ManagedId,
    },
    /// A non-retaining edge.
    Weak {
        /// Stable edge identity.
        edge: EdgeId,
        /// Non-retained target.
        target: ManagedId,
    },
    /// An ephemeron key/value pair.
    Ephemeron {
        /// Stable edge identity.
        edge: EdgeId,
        /// Conditional-retention key.
        key: ManagedId,
        /// Conditionally retained value.
        value: ManagedId,
    },
}

impl EdgeSnapshot {
    /// Returns the stable edge identity independent of edge kind.
    pub const fn id(self) -> EdgeId {
        match self {
            Self::Strong { edge, .. } | Self::Weak { edge, .. } | Self::Ephemeron { edge, .. } => {
                edge
            }
        }
    }
}

/// An edge identity paired with its immutable collection semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TypedEdgeId {
    id: EdgeId,
    kind: EdgeKind,
}

impl TypedEdgeId {
    /// Returns the stable identity.
    pub const fn id(self) -> EdgeId {
        self.id
    }

    /// Returns the edge's collection semantics.
    pub const fn kind(self) -> EdgeKind {
        self.kind
    }
}

/// Fail-closed edge allocation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeAllocationError {
    /// The per-object edge identity space is exhausted.
    IdentityExhausted,
    /// The configured total or per-kind edge cap was reached.
    CapacityExceeded {
        /// Kind requested by the refused insertion.
        kind: EdgeKind,
        /// Applicable total or per-kind cap.
        cap: usize,
    },
}

impl fmt::Display for EdgeAllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityExhausted => f.write_str("managed edge identity space exhausted"),
            Self::CapacityExceeded { kind, cap } => {
                write!(f, "managed {kind:?} edge cap {cap} reached")
            }
        }
    }
}

impl Error for EdgeAllocationError {}

/// Monotonic, per-object allocation of stable edge identities.
///
/// Removing an edge is deliberately not an allocator operation: allocated
/// identities are evidence and remain consumed for the lifetime of their
/// owner. Once the `u32` identity space is exhausted, every later allocation
/// fails without changing allocator state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeAllocator {
    next: Option<u32>,
}

impl EdgeAllocator {
    /// Creates an allocator whose first identity has ordinal zero.
    pub const fn new() -> Self {
        Self { next: Some(0) }
    }

    /// Allocates the next identity with immutable `kind` semantics.
    pub fn allocate(&mut self, kind: EdgeKind) -> Result<TypedEdgeId, EdgeAllocationError> {
        let ordinal = self.next.ok_or(EdgeAllocationError::IdentityExhausted)?;
        self.next = ordinal.checked_add(1);
        Ok(TypedEdgeId {
            id: EdgeId(ordinal),
            kind,
        })
    }

    #[cfg(test)]
    pub(crate) const fn starting_at(next: u32) -> Self {
        Self { next: Some(next) }
    }
}

impl Default for EdgeAllocator {
    fn default() -> Self {
        Self::new()
    }
}

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

/// A role-bearing managed object with stable, ordered strong edges.
///
/// Edge storage and identity allocation are deliberately private. Callers can
/// mutate the graph only through checked operations, so identities are never
/// reused and compare-and-mutate failures leave the node unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedNode<R> {
    role: ManagedRole<R>,
    edges: EdgeAllocator,
    limits: EdgeLimits,
    strong: BTreeMap<EdgeId, ManagedId>,
    weak: BTreeMap<EdgeId, ManagedId>,
    ephemerons: BTreeMap<EdgeId, (ManagedId, ManagedId)>,
}

/// A managed object carrying caller-owned role evidence outside graph policy.
pub trait RoleBearingManagedObject: ManagedObject {
    /// Role label type chosen by the object owner.
    type Role;
    /// Borrows the current audit role.
    fn managed_role(&self) -> &Self::Role;
    /// Replaces audit role evidence without changing the managed graph.
    fn replace_managed_role(&mut self, role: Self::Role) -> Self::Role;
}

impl<R> ManagedNode<R> {
    /// Constructs an empty node carrying caller-owned role evidence.
    pub const fn new(role: R) -> Self {
        Self::with_edge_limits(role, EdgeLimits::DEFAULT)
    }

    /// Constructs an empty node with explicit total and per-kind hard caps.
    pub const fn with_edge_limits(role: R, limits: EdgeLimits) -> Self {
        Self {
            role: ManagedRole::new(role),
            edges: EdgeAllocator::new(),
            limits,
            strong: BTreeMap::new(),
            weak: BTreeMap::new(),
            ephemerons: BTreeMap::new(),
        }
    }

    /// Returns an allocation-ordered copy of every live outgoing edge.
    pub fn edge_snapshot(&self) -> Vec<EdgeSnapshot> {
        let mut result = self
            .strong
            .iter()
            .map(|(&edge, &target)| EdgeSnapshot::Strong { edge, target })
            .chain(
                self.weak
                    .iter()
                    .map(|(&edge, &target)| EdgeSnapshot::Weak { edge, target }),
            )
            .chain(
                self.ephemerons
                    .iter()
                    .map(|(&edge, &(key, value))| EdgeSnapshot::Ephemeron { edge, key, value }),
            )
            .collect::<Vec<_>>();
        result.sort_unstable_by_key(|entry| entry.id());
        result
    }

    fn edge_kind(&self, edge: EdgeId) -> Option<EdgeKind> {
        self.strong
            .contains_key(&edge)
            .then_some(EdgeKind::Strong)
            .or_else(|| self.weak.contains_key(&edge).then_some(EdgeKind::Weak))
            .or_else(|| {
                self.ephemerons
                    .contains_key(&edge)
                    .then_some(EdgeKind::Ephemeron)
            })
    }

    fn admit(&self, kind: EdgeKind) -> Result<(), EdgeAllocationError> {
        let total = self.strong.len() + self.weak.len() + self.ephemerons.len();
        if total >= self.limits.total() {
            return Err(EdgeAllocationError::CapacityExceeded {
                kind,
                cap: self.limits.total(),
            });
        }
        let count = match kind {
            EdgeKind::Strong => self.strong.len(),
            EdgeKind::Weak => self.weak.len(),
            EdgeKind::Ephemeron => self.ephemerons.len(),
        };
        let cap = self.limits.for_kind(kind);
        if count >= cap {
            return Err(EdgeAllocationError::CapacityExceeded { kind, cap });
        }
        Ok(())
    }

    /// Borrows the node's role evidence.
    pub const fn role(&self) -> &R {
        self.role.role()
    }

    /// Replaces the role without changing the managed graph.
    pub fn replace_role(&mut self, role: R) -> R {
        self.role.replace_role(role)
    }

    /// Inserts a strong edge and returns its stable identity.
    pub fn insert_strong(&mut self, target: ManagedId) -> Result<EdgeId, StrongEdgeMutationError> {
        self.admit(EdgeKind::Strong)?;
        let edge = self.edges.allocate(EdgeKind::Strong)?.id();
        let previous = self.strong.insert(edge, target);
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces a strong target only if it still equals `expected`.
    pub fn replace_strong(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
        replacement: ManagedId,
    ) -> Result<(), StrongEdgeMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Strong)
        {
            return Err(StrongEdgeMutationError::WrongKind { edge, actual });
        }
        let target = self
            .strong
            .get_mut(&edge)
            .ok_or(StrongEdgeMutationError::UnknownEdge(edge))?;
        if *target != expected {
            return Err(StrongEdgeMutationError::TargetChanged {
                expected,
                actual: *target,
            });
        }
        *target = replacement;
        Ok(())
    }

    /// Removes a strong edge only if it still equals `expected`.
    pub fn remove_strong(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
    ) -> Result<ManagedId, StrongEdgeMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Strong)
        {
            return Err(StrongEdgeMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .strong
            .get(&edge)
            .copied()
            .ok_or(StrongEdgeMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(StrongEdgeMutationError::TargetChanged { expected, actual });
        }
        Ok(self
            .strong
            .remove(&edge)
            .expect("edge checked immediately before removal"))
    }

    /// Inserts a weak edge and returns its stable identity.
    pub fn insert_weak(&mut self, target: ManagedId) -> Result<EdgeId, WeakEdgeMutationError> {
        self.admit(EdgeKind::Weak)?;
        let edge = self.edges.allocate(EdgeKind::Weak)?.id();
        let previous = self.weak.insert(edge, target);
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces a weak target only if it still equals `expected`.
    pub fn replace_weak(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
        replacement: ManagedId,
    ) -> Result<(), WeakEdgeMutationError> {
        if let Some(actual) = self.edge_kind(edge).filter(|kind| *kind != EdgeKind::Weak) {
            return Err(WeakEdgeMutationError::WrongKind { edge, actual });
        }
        let target = self
            .weak
            .get_mut(&edge)
            .ok_or(WeakEdgeMutationError::UnknownEdge(edge))?;
        if *target != expected {
            return Err(WeakEdgeMutationError::TargetChanged {
                expected,
                actual: *target,
            });
        }
        *target = replacement;
        Ok(())
    }

    /// Removes a weak edge only if it still equals `expected`.
    pub fn remove_weak(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
    ) -> Result<ManagedId, WeakEdgeMutationError> {
        if let Some(actual) = self.edge_kind(edge).filter(|kind| *kind != EdgeKind::Weak) {
            return Err(WeakEdgeMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .weak
            .get(&edge)
            .copied()
            .ok_or(WeakEdgeMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(WeakEdgeMutationError::TargetChanged { expected, actual });
        }
        Ok(self
            .weak
            .remove(&edge)
            .expect("edge checked immediately before removal"))
    }

    /// Inserts an ephemeron entry and returns its stable identity.
    pub fn insert_ephemeron(
        &mut self,
        key: ManagedId,
        value: ManagedId,
    ) -> Result<EdgeId, EphemeronMutationError> {
        self.admit(EdgeKind::Ephemeron)?;
        let edge = self.edges.allocate(EdgeKind::Ephemeron)?.id();
        let previous = self.ephemerons.insert(edge, (key, value));
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces an ephemeron only if it still contains the expected pair.
    pub fn replace_ephemeron(
        &mut self,
        edge: EdgeId,
        expected: (ManagedId, ManagedId),
        replacement: (ManagedId, ManagedId),
    ) -> Result<(), EphemeronMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Ephemeron)
        {
            return Err(EphemeronMutationError::WrongKind { edge, actual });
        }
        let entry = self
            .ephemerons
            .get_mut(&edge)
            .ok_or(EphemeronMutationError::UnknownEdge(edge))?;
        if *entry != expected {
            return Err(EphemeronMutationError::EntryChanged {
                expected_key: expected.0,
                expected_value: expected.1,
                actual_key: entry.0,
                actual_value: entry.1,
            });
        }
        *entry = replacement;
        Ok(())
    }

    /// Removes an ephemeron only if it still contains the expected pair.
    pub fn remove_ephemeron(
        &mut self,
        edge: EdgeId,
        expected: (ManagedId, ManagedId),
    ) -> Result<(ManagedId, ManagedId), EphemeronMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Ephemeron)
        {
            return Err(EphemeronMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .ephemerons
            .get(&edge)
            .copied()
            .ok_or(EphemeronMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(EphemeronMutationError::EntryChanged {
                expected_key: expected.0,
                expected_value: expected.1,
                actual_key: actual.0,
                actual_value: actual.1,
            });
        }
        Ok(self
            .ephemerons
            .remove(&edge)
            .expect("entry checked immediately before removal"))
    }
}

impl<R> ManagedObject for ManagedNode<R> {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        let mut edges = self
            .strong
            .iter()
            .map(|(&edge, &target)| (edge, EdgeKind::Strong, target, target))
            .chain(
                self.weak
                    .iter()
                    .map(|(&edge, &target)| (edge, EdgeKind::Weak, target, target)),
            )
            .chain(
                self.ephemerons
                    .iter()
                    .map(|(&edge, &(key, value))| (edge, EdgeKind::Ephemeron, key, value)),
            )
            .collect::<Vec<_>>();
        edges.sort_unstable_by_key(|entry| entry.0);
        for (edge, kind, first, second) in edges {
            match kind {
                EdgeKind::Strong => visitor.strong(edge, first),
                EdgeKind::Weak => visitor.weak(edge, first),
                EdgeKind::Ephemeron => visitor.ephemeron(edge, first, second),
            }
        }
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        if self.weak.get(&edge) != Some(&expected) {
            return false;
        }
        self.weak.remove(&edge).is_some()
    }

    fn clear_ephemeron_edge(
        &mut self,
        edge: EdgeId,
        expected_key: ManagedId,
        expected_value: ManagedId,
    ) -> bool {
        if self.ephemerons.get(&edge) != Some(&(expected_key, expected_value)) {
            return false;
        }
        self.ephemerons.remove(&edge).is_some()
    }
}

impl<R> RoleBearingManagedObject for ManagedNode<R> {
    type Role = R;

    fn managed_role(&self) -> &Self::Role {
        self.role()
    }

    fn replace_managed_role(&mut self, role: Self::Role) -> Self::Role {
        self.replace_role(role)
    }
}

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

/// Bounded storage for managed objects, independent of language and collector policy.
pub struct ManagedArena<T> {
    policy: HardCappedRetainPolicy,
    next_id: u64,
    next_root: u64,
    next_safepoint: u64,
    mutation_epoch: u64,
    objects: BTreeMap<ManagedId, T>,
    roots: BTreeMap<RootId, ManagedId>,
    kept_alive: BTreeMap<ManagedId, u64>,
}

impl<T> ManagedArena<T> {
    /// Creates an empty arena using the hard-capped retain policy.
    pub fn new(policy: HardCappedRetainPolicy) -> Self {
        Self {
            policy,
            next_id: 0,
            next_root: 0,
            next_safepoint: 0,
            mutation_epoch: 0,
            objects: BTreeMap::new(),
            roots: BTreeMap::new(),
            kept_alive: BTreeMap::new(),
        }
    }

    /// Returns the tracing contract version.
    pub const fn trace_contract_version(&self) -> TraceContractVersion {
        TraceContractVersion::V1
    }

    /// Returns the number of live objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Reports whether the arena contains no objects.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Returns the epoch advanced by every graph-affecting arena mutation.
    pub const fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    fn advance_mutation_epoch(&mut self) -> Result<(), ArenaError> {
        self.mutation_epoch = self
            .mutation_epoch
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        Ok(())
    }

    /// Allocates atomically after checking the cap and identity space.
    pub fn allocate(&mut self, object: T) -> Result<ManagedHandle, ArenaError> {
        if self.objects.len() >= self.policy.max_objects {
            return Err(ArenaError::CapacityExceeded {
                cap: self.policy.max_objects,
            });
        }
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let id = ManagedId(self.next_id);
        self.advance_mutation_epoch()?;
        self.objects.insert(id, object);
        self.next_id = next;
        Ok(ManagedHandle { id })
    }

    /// Returns a shared object reference, refusing stale handles.
    pub fn get(&self, handle: ManagedHandle) -> Result<&T, ArenaError> {
        self.objects
            .get(&handle.id)
            .ok_or(ArenaError::StaleHandle(handle.id))
    }

    /// Returns a mutable object reference, refusing stale handles.
    pub fn get_mut(&mut self, handle: ManagedHandle) -> Result<&mut T, ArenaError> {
        if !self.objects.contains_key(&handle.id) {
            return Err(ArenaError::StaleHandle(handle.id));
        }
        self.advance_mutation_epoch()?;
        Ok(self
            .objects
            .get_mut(&handle.id)
            .expect("validated managed id"))
    }

    /// Upgrades a weak handle only while its object remains live.
    pub fn upgrade(&mut self, weak: WeakHandle) -> Result<ManagedHandle, ArenaError> {
        if !self.objects.contains_key(&weak.id) {
            return Err(ArenaError::StaleHandle(weak.id));
        }
        self.kept_alive.insert(weak.id, self.mutation_epoch);
        Ok(ManagedHandle { id: weak.id })
    }

    /// Resolves a tracing identity to a live handle for collector operations.
    pub fn handle(&self, id: ManagedId) -> Result<ManagedHandle, ArenaError> {
        self.objects
            .contains_key(&id)
            .then_some(ManagedHandle { id })
            .ok_or(ArenaError::StaleHandle(id))
    }

    /// Registers a root after validating the handle.
    pub fn root(&mut self, handle: ManagedHandle) -> Result<RootedHandle, ArenaError> {
        self.get(handle)?;
        let next = self
            .next_root
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let root = RootId(self.next_root);
        self.advance_mutation_epoch()?;
        self.roots.insert(root, handle.id);
        self.next_root = next;
        Ok(RootedHandle { root, handle })
    }

    /// Releases exactly one matching root registration.
    pub fn release_root(&mut self, rooted: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        match self.roots.get(&rooted.root) {
            Some(id) if *id == rooted.handle.id => {
                self.advance_mutation_epoch()?;
                self.roots.remove(&rooted.root);
                Ok(rooted.handle)
            }
            _ => Err(ArenaError::StaleRoot(rooted.root)),
        }
    }

    /// Removes an unrooted object, making all handles to it stale.
    pub fn remove(&mut self, handle: ManagedHandle) -> Result<T, ArenaError> {
        if self.roots.values().any(|id| *id == handle.id) {
            return Err(ArenaError::ObjectRooted(handle.id));
        }
        if !self.objects.contains_key(&handle.id) {
            return Err(ArenaError::StaleHandle(handle.id));
        }
        self.advance_mutation_epoch()?;
        let removed = self
            .objects
            .remove(&handle.id)
            .expect("validated managed id");
        Ok(removed)
    }

    /// Clears a weak edge through the owning object's at-most-once operation.
    pub fn clear_weak_edge(
        &mut self,
        owner: ManagedHandle,
        edge: EdgeId,
        expected: WeakHandle,
    ) -> Result<bool, ArenaError>
    where
        T: ManagedObject,
    {
        if !self.objects.contains_key(&owner.id) {
            return Err(ArenaError::StaleHandle(owner.id));
        }
        self.advance_mutation_epoch()?;
        let cleared = self
            .objects
            .get_mut(&owner.id)
            .expect("validated managed id")
            .clear_weak_edge(edge, expected.id);
        Ok(cleared)
    }

    /// Atomically removes an allocation-ordered set selected from `expected_epoch`.
    ///
    /// Every identity and root condition is checked before the first slot changes.
    pub fn sweep_at_epoch(
        &mut self,
        expected_epoch: u64,
        objects: &[ManagedId],
    ) -> Result<Vec<ManagedId>, ArenaError> {
        if self.mutation_epoch != expected_epoch {
            return Err(ArenaError::MutationEpochChanged {
                expected: expected_epoch,
                actual: self.mutation_epoch,
            });
        }
        for id in objects {
            if !self.objects.contains_key(id) {
                return Err(ArenaError::StaleHandle(*id));
            }
            if self.roots.values().any(|rooted| rooted == id) {
                return Err(ArenaError::ObjectRooted(*id));
            }
        }
        if !objects.is_empty() {
            self.advance_mutation_epoch()?;
        }
        for id in objects {
            self.objects.remove(id);
        }
        Ok(objects.to_vec())
    }

    /// Applies a collector plan atomically at `expected_epoch`.
    ///
    /// Kept-alive objects from that epoch are retained. Weak and ephemeron
    /// entries are cleared before unreachable objects are removed, and every
    /// conditional clear is intrinsically at most once.
    pub fn apply_collection_at_epoch(
        &mut self,
        expected_epoch: u64,
        weak: &[(ManagedId, EdgeId, ManagedId)],
        ephemerons: &[(ManagedId, EdgeId, ManagedId, ManagedId)],
        swept: &[ManagedId],
    ) -> Result<CollectionMutationReceipt, ArenaError>
    where
        T: ManagedObject,
    {
        if self.mutation_epoch != expected_epoch {
            return Err(ArenaError::MutationEpochChanged {
                expected: expected_epoch,
                actual: self.mutation_epoch,
            });
        }
        let kept = self
            .kept_alive
            .iter()
            .filter_map(|(id, epoch)| (*epoch == expected_epoch).then_some(*id))
            .collect::<std::collections::BTreeSet<_>>();
        let actual_swept = swept
            .iter()
            .copied()
            .filter(|id| !kept.contains(id))
            .collect::<Vec<_>>();
        for id in &actual_swept {
            if !self.objects.contains_key(id) {
                return Err(ArenaError::StaleHandle(*id));
            }
            if self.roots.values().any(|rooted| rooted == id) {
                return Err(ArenaError::ObjectRooted(*id));
            }
        }
        if !weak.is_empty() || !ephemerons.is_empty() || !actual_swept.is_empty() {
            self.advance_mutation_epoch()?;
        }
        let mut cleared_weak = Vec::new();
        for &(owner, edge, target) in weak {
            if let Some(object) = self.objects.get_mut(&owner)
                && object.clear_weak_edge(edge, target)
            {
                cleared_weak.push((owner, edge));
            }
        }
        let mut cleared_ephemerons = Vec::new();
        for &(owner, edge, key, value) in ephemerons {
            if let Some(object) = self.objects.get_mut(&owner)
                && object.clear_ephemeron_edge(edge, key, value)
            {
                cleared_ephemerons.push((owner, edge));
            }
        }
        for id in &actual_swept {
            self.objects.remove(id);
        }
        self.kept_alive
            .retain(|id, epoch| self.objects.contains_key(id) && *epoch != expected_epoch);
        Ok(CollectionMutationReceipt {
            cleared_weak,
            cleared_ephemerons,
            swept: actual_swept,
        })
    }

    /// Runs a read-only tracing callback at a deterministic safepoint.
    pub fn safepoint<R>(
        &mut self,
        trace: impl FnOnce(&TraceSnapshot<'_, T>) -> R,
    ) -> Result<(R, SafepointReceipt), ArenaError>
    where
        T: ManagedObject,
    {
        let next = self
            .next_safepoint
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let roots = self.roots.values().copied().collect::<Vec<_>>();
        let snapshot = TraceSnapshot {
            roots: roots.clone(),
            kept_alive: self
                .kept_alive
                .iter()
                .filter_map(|(id, epoch)| (*epoch == self.mutation_epoch).then_some(*id))
                .collect(),
            objects: &self.objects,
            mutation_epoch: self.mutation_epoch,
        };
        let result = trace(&snapshot);
        let receipt = SafepointReceipt {
            sequence: self.next_safepoint,
            roots,
            objects: self.objects.keys().copied().collect(),
        };
        self.next_safepoint = next;
        Ok((result, receipt))
    }

    /// Tears down all storage and roots, returning allocation-ordered evidence.
    pub fn teardown(&mut self) -> TeardownReceipt {
        let receipt = TeardownReceipt {
            objects: self.objects.keys().copied().collect(),
            roots: self.roots.keys().copied().collect(),
        };
        if !self.objects.is_empty() || !self.roots.is_empty() {
            self.mutation_epoch = self.mutation_epoch.saturating_add(1);
        }
        self.objects.clear();
        self.roots.clear();
        self.kept_alive.clear();
        receipt
    }
}

impl<T: RoleBearingManagedObject> ManagedArena<T> {
    /// Replaces caller-owned role evidence without advancing the graph epoch.
    pub fn replace_role(
        &mut self,
        handle: ManagedHandle,
        role: T::Role,
    ) -> Result<T::Role, ArenaError> {
        self.objects
            .get_mut(&handle.id)
            .map(|object| object.replace_managed_role(role))
            .ok_or(ArenaError::StaleHandle(handle.id))
    }

    /// Projects optional owner-role evidence at a read-only safepoint.
    ///
    /// Admission is checked before invoking `role`, and rows follow managed id
    /// order. The projection observes objects but is not visible to tracing or
    /// collector policy.
    pub fn project_roles(
        &mut self,
        limit: usize,
    ) -> Result<RoleProjectionReceipt<T::Role>, RoleProjectionError>
    where
        T::Role: Clone,
    {
        let (roles, safepoint) = self.safepoint(|snapshot| {
            let required = snapshot.objects.len();
            if required > limit {
                return Err(RoleProjectionError::Limit { limit, required });
            }
            let roles = snapshot
                .objects
                .iter()
                .map(|(&id, object)| (id, object.managed_role().clone()))
                .collect();
            Ok((snapshot.mutation_epoch(), roles))
        })?;
        let (mutation_epoch, roles) = roles?;
        Ok(RoleProjectionReceipt {
            safepoint,
            mutation_epoch,
            roles,
        })
    }
}
