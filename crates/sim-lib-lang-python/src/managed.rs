use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, collect};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedHandle, ManagedId,
    ManagedObject,
};

/// Cyclic mutable Python payload held exclusively in the shared managed arena.
#[derive(Clone, Debug, Default)]
pub struct PythonManagedObject {
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
}
