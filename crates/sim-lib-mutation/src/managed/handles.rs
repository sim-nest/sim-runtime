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
