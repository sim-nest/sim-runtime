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
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct EdgeId(pub u32);

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
        }
    }
}

impl Error for ArenaError {}

/// An immutable, complete tracing view taken at a safepoint.
pub struct TraceSnapshot<'a, T> {
    roots: Vec<ManagedId>,
    objects: &'a BTreeMap<ManagedId, T>,
}

impl<T: ManagedObject> TraceSnapshot<'_, T> {
    /// Enumerates roots in root-registration order.
    pub fn roots(&self) -> impl ExactSizeIterator<Item = ManagedId> + '_ {
        self.roots.iter().copied()
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

/// Deterministic evidence returned by explicit arena teardown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TeardownReceipt {
    /// Objects removed in allocation order.
    pub objects: Vec<ManagedId>,
    /// Root registrations removed in registration order.
    pub roots: Vec<RootId>,
}

/// Bounded storage for managed objects, independent of language and collector policy.
pub struct ManagedArena<T> {
    policy: HardCappedRetainPolicy,
    next_id: u64,
    next_root: u64,
    next_safepoint: u64,
    objects: BTreeMap<ManagedId, T>,
    roots: BTreeMap<RootId, ManagedId>,
}

impl<T> ManagedArena<T> {
    /// Creates an empty arena using the hard-capped retain policy.
    pub fn new(policy: HardCappedRetainPolicy) -> Self {
        Self {
            policy,
            next_id: 0,
            next_root: 0,
            next_safepoint: 0,
            objects: BTreeMap::new(),
            roots: BTreeMap::new(),
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
        self.objects
            .get_mut(&handle.id)
            .ok_or(ArenaError::StaleHandle(handle.id))
    }

    /// Upgrades a weak handle only while its object remains live.
    pub fn upgrade(&self, weak: WeakHandle) -> Result<ManagedHandle, ArenaError> {
        self.objects
            .contains_key(&weak.id)
            .then_some(ManagedHandle { id: weak.id })
            .ok_or(ArenaError::StaleHandle(weak.id))
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
        self.roots.insert(root, handle.id);
        self.next_root = next;
        Ok(RootedHandle { root, handle })
    }

    /// Releases exactly one matching root registration.
    pub fn release_root(&mut self, rooted: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        match self.roots.get(&rooted.root) {
            Some(id) if *id == rooted.handle.id => {
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
        self.objects
            .remove(&handle.id)
            .ok_or(ArenaError::StaleHandle(handle.id))
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
        Ok(self.get_mut(owner)?.clear_weak_edge(edge, expected.id))
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
            objects: &self.objects,
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
        self.objects.clear();
        self.roots.clear();
        receipt
    }
}
