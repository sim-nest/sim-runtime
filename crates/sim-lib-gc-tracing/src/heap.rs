use sim_lib_mutation::{
    ArenaError, HardCappedRetainPolicy, ManagedArena, ManagedHandle, ManagedId, ManagedObject,
    RootedHandle, TeardownReceipt,
};

use crate::{CollectionError, CollectionLimits, CollectionReceipt, collect};

/// Explicit reclamation policy for a managed heap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedHeapPolicy {
    /// Reclaim unreachable objects with the bounded tracing collector.
    Tracing(CollectionLimits),
    /// Retain every object until explicit heap teardown.
    Retain,
}

/// A dependency-correct managed heap composed from an arena and reclamation policy.
///
/// The wrapper is generic over the guest payload and lives beside the collector,
/// so guest runtimes do not need to duplicate policy or lifecycle behavior.
pub struct ManagedHeap<T: ManagedObject> {
    arena: ManagedArena<T>,
    policy: ManagedHeapPolicy,
}

impl<T: ManagedObject> ManagedHeap<T> {
    /// Creates a heap which uses bounded tracing collection.
    pub fn tracing(cap: usize, limits: CollectionLimits) -> Result<Self, ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: ManagedHeapPolicy::Tracing(limits),
        })
    }

    /// Creates a heap which retains objects until explicit teardown.
    pub fn retaining(cap: usize) -> Result<Self, ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: ManagedHeapPolicy::Retain,
        })
    }

    /// Allocates a value after checking the arena capacity and identity space.
    pub fn allocate(&mut self, value: T) -> Result<ManagedHandle, ArenaError> {
        self.arena.allocate(value)
    }

    /// Returns a shared value reference after validating its handle.
    pub fn get(&self, handle: ManagedHandle) -> Result<&T, ArenaError> {
        self.arena.get(handle)
    }

    /// Returns a mutable value reference after validating its handle.
    pub fn get_mut(&mut self, handle: ManagedHandle) -> Result<&mut T, ArenaError> {
        self.arena.get_mut(handle)
    }

    /// Resolves a live managed identity to its generation-checked handle.
    pub fn handle(&self, id: ManagedId) -> Result<ManagedHandle, ArenaError> {
        self.arena.handle(id)
    }

    /// Registers a validated handle as a tracing root.
    pub fn root(&mut self, handle: ManagedHandle) -> Result<RootedHandle, ArenaError> {
        self.arena.root(handle)
    }

    /// Releases one matching root registration.
    pub fn release_root(&mut self, rooted: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        self.arena.release_root(rooted)
    }

    /// Returns the number of live managed allocations.
    pub fn live_len(&self) -> usize {
        self.arena.len()
    }

    /// Returns the selected reclamation policy.
    pub const fn policy(&self) -> ManagedHeapPolicy {
        self.policy
    }

    /// Describes the cycle-reclamation gap when retention is selected.
    pub const fn cycle_leak_gap(&self) -> Option<&'static str> {
        match self.policy {
            ManagedHeapPolicy::Tracing(_) => None,
            ManagedHeapPolicy::Retain => {
                Some("unreachable strong cycles are retained until heap teardown")
            }
        }
    }

    /// Runs the configured reclamation policy at a safepoint.
    ///
    /// Retaining heaps return `None` without mutating the arena.
    pub fn collect(&mut self) -> Result<Option<CollectionReceipt>, CollectionError> {
        match self.policy {
            ManagedHeapPolicy::Tracing(limits) => collect(&mut self.arena, limits).map(Some),
            ManagedHeapPolicy::Retain => Ok(None),
        }
    }

    /// Removes every object and root, returning deterministic teardown evidence.
    pub fn teardown(&mut self) -> TeardownReceipt {
        self.arena.teardown()
    }
}

#[cfg(test)]
mod tests {
    use sim_lib_mutation::{EdgeId, EdgeVisitor, ManagedId};

    use super::*;

    #[derive(Default)]
    struct Node(Vec<ManagedId>);

    impl ManagedObject for Node {
        fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
            for (edge, target) in self.0.iter().copied().enumerate() {
                visitor.strong(EdgeId(edge as u32), target);
            }
        }

        fn clear_weak_edge(&mut self, _: EdgeId, _: ManagedId) -> bool {
            false
        }
    }

    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 8,
            edges: 8,
            stack: 8,
            work: 32,
            clears: 8,
            finalizers: 0,
        }
    }

    #[test]
    fn tracing_and_retaining_policies_match_guest_heap_behavior() {
        let tracing = ManagedHeap::<Node>::tracing(8, limits()).unwrap();
        assert_eq!(tracing.policy(), ManagedHeapPolicy::Tracing(limits()));
        assert_eq!(tracing.cycle_leak_gap(), None);

        let mut retaining = ManagedHeap::<Node>::retaining(8).unwrap();
        retaining.allocate(Node::default()).unwrap();
        assert_eq!(retaining.policy(), ManagedHeapPolicy::Retain);
        assert!(retaining.cycle_leak_gap().unwrap().contains("cycles"));
        assert_eq!(retaining.collect().unwrap(), None);
        assert_eq!(retaining.live_len(), 1);
    }

    #[test]
    fn tracing_reclaims_cycles_and_checked_access_rejects_stale_handles() {
        let mut heap = ManagedHeap::tracing(8, limits()).unwrap();
        let first = heap.allocate(Node::default()).unwrap();
        let second = heap.allocate(Node::default()).unwrap();
        assert_eq!(heap.handle(first.id()).unwrap(), first);
        heap.get_mut(first).unwrap().0.push(second.id());
        heap.get_mut(second).unwrap().0.push(first.id());

        assert_eq!(
            heap.collect().unwrap().unwrap().swept,
            [first.id(), second.id()]
        );
        assert!(matches!(heap.get(first), Err(ArenaError::StaleHandle(id)) if id == first.id()));
        assert!(
            matches!(heap.handle(first.id()), Err(ArenaError::StaleHandle(id)) if id == first.id())
        );
        assert_eq!(heap.live_len(), 0);
    }

    #[test]
    fn roots_survive_collection_and_teardown_reports_all_state() {
        let mut heap = ManagedHeap::tracing(8, limits()).unwrap();
        let handle = heap.allocate(Node::default()).unwrap();
        let rooted = heap.root(handle).unwrap();
        assert!(heap.collect().unwrap().unwrap().swept.is_empty());

        let receipt = heap.teardown();
        assert_eq!(receipt.objects, [handle.id()]);
        assert_eq!(receipt.roots, [rooted.root_id()]);
        assert_eq!(heap.live_len(), 0);
    }
}
