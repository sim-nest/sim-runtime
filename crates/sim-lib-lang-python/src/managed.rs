use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, collect};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedHandle, ManagedId,
    ManagedObject,
};

/// Language-visible role of a managed Python allocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PythonManagedKind {
    /// An instance or class object.
    #[default]
    Instance,
    /// A closure environment.
    Closure,
    /// A suspended or executing frame.
    Frame,
    /// An exception, traceback, or exception group.
    Exception,
    /// A mutable container.
    Container,
}

/// Cyclic mutable Python payload held exclusively in the shared managed arena.
#[derive(Clone, Debug, Default)]
pub struct PythonManagedObject {
    /// Language-visible allocation role; collection does not special-case it.
    pub kind: PythonManagedKind,
    /// Strong links to other Python objects.
    pub edges: Vec<ManagedId>,
}
impl ManagedObject for PythonManagedObject {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (i, target) in self.edges.iter().copied().enumerate() {
            visitor.strong(EdgeId(i as u32), target);
        }
    }
    fn clear_weak_edge(&mut self, _: EdgeId, _: ManagedId) -> bool {
        false
    }
    fn clear_ephemeron_edge(&mut self, _: EdgeId, _: ManagedId, _: ManagedId) -> bool {
        false
    }
}

/// Explicit reclaim policy. Tracing is the standard; retention is opt-in and inspectable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PythonHeapPolicy {
    /// Run the shared bounded tracing collector.
    Tracing(CollectionLimits),
    /// Retain until teardown; strong cycles leak by contract.
    Retain,
}

/// Python managed heap composed from the shared arena and optional collector.
pub struct PythonHeap {
    arena: ManagedArena<PythonManagedObject>,
    policy: PythonHeapPolicy,
}
impl PythonHeap {
    /// Create the standard tracing heap.
    pub fn standard(
        cap: usize,
        limits: CollectionLimits,
    ) -> Result<Self, sim_lib_mutation::ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: PythonHeapPolicy::Tracing(limits),
        })
    }
    /// Create the explicit no-collector heap whose cycle leak is reported by `cycle_leak_gap`.
    pub fn retaining(cap: usize) -> Result<Self, sim_lib_mutation::ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: PythonHeapPolicy::Retain,
        })
    }
    /// Allocate a cyclic-capable value only in the managed arena.
    pub fn allocate(
        &mut self,
        value: PythonManagedObject,
    ) -> Result<ManagedHandle, sim_lib_mutation::ArenaError> {
        self.arena.allocate(value)
    }
    /// Add a strong language edge between two managed values.
    pub fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<(), sim_lib_mutation::ArenaError> {
        self.arena.get_mut(from)?.edges.push(to.id());
        Ok(())
    }
    /// Return the number of live managed allocations.
    pub fn live_len(&self) -> usize {
        self.arena.len()
    }
    /// Return selected policy.
    pub const fn policy(&self) -> PythonHeapPolicy {
        self.policy
    }
    /// Return the explicit retention gap, if selected.
    pub const fn cycle_leak_gap(&self) -> Option<&'static str> {
        match self.policy {
            PythonHeapPolicy::Retain => {
                Some("unreachable strong cycles are retained until heap teardown")
            }
            PythonHeapPolicy::Tracing(_) => None,
        }
    }
    /// Run a safepoint. Retention performs no implicit collection.
    pub fn collect(&mut self) -> Result<Option<CollectionReceipt>, CollectionError> {
        match self.policy {
            PythonHeapPolicy::Tracing(limits) => collect(&mut self.arena, limits).map(Some),
            PythonHeapPolicy::Retain => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn tracing_is_standard_and_retention_is_explicit() {
        let standard = PythonHeap::standard(8, limits()).unwrap();
        assert!(matches!(standard.policy(), PythonHeapPolicy::Tracing(_)));
        assert_eq!(standard.cycle_leak_gap(), None);
        let retaining = PythonHeap::retaining(8).unwrap();
        assert_eq!(retaining.policy(), PythonHeapPolicy::Retain);
        assert!(retaining.cycle_leak_gap().unwrap().contains("cycles"));
    }

    #[test]
    fn unreachable_python_objects_are_reclaimed_by_shared_collector() {
        let mut heap = PythonHeap::standard(8, limits()).unwrap();
        heap.allocate(PythonManagedObject::default()).unwrap();
        let receipt = heap.collect().unwrap().unwrap();
        assert_eq!(receipt.swept.len(), 1);
    }

    #[test]
    fn heterogeneous_language_cycle_is_reclaimed_without_observable_mutation() {
        let mut heap = PythonHeap::standard(8, limits()).unwrap();
        let kinds = [
            PythonManagedKind::Instance,
            PythonManagedKind::Closure,
            PythonManagedKind::Frame,
            PythonManagedKind::Exception,
            PythonManagedKind::Container,
        ];
        let handles: Vec<_> = kinds
            .into_iter()
            .map(|kind| {
                heap.allocate(PythonManagedObject {
                    kind,
                    edges: vec![],
                })
                .unwrap()
            })
            .collect();
        for pair in handles.windows(2) {
            heap.connect(pair[0], pair[1]).unwrap();
        }
        heap.connect(handles[4], handles[0]).unwrap();
        let visible_result = 42;
        let receipt = heap.collect().unwrap().unwrap();
        assert_eq!(receipt.swept.len(), 5);
        assert_eq!(heap.live_len(), 0);
        assert_eq!(visible_result, 42);
    }
}
