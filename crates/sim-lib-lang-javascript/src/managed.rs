use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, collect};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedHandle, ManagedId,
    ManagedObject,
};

/// Single-agent role of a managed JavaScript allocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JavascriptManagedKind {
    /// Ordinary cyclic object.
    #[default]
    Object,
    /// Lexical environment or closure.
    Environment,
    /// Iterator or suspended frame.
    Frame,
}
/// Cyclic JavaScript payload stored exclusively by the shared arena.
#[derive(Clone, Debug, Default)]
pub struct JavascriptManagedObject {
    /// Language role.
    pub kind: JavascriptManagedKind,
    /// Strong language edges.
    pub edges: Vec<ManagedId>,
}
impl ManagedObject for JavascriptManagedObject {
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
/// Explicit collection policy; collection is optional, never a load prerequisite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptHeapPolicy {
    /// Shared tracing collector.
    Tracing(CollectionLimits),
    /// Retain until teardown, with an inspectable cycle gap.
    Retain,
}
/// JavaScript cyclic state composed from the shared arena and collector.
pub struct JavascriptHeap {
    arena: ManagedArena<JavascriptManagedObject>,
    policy: JavascriptHeapPolicy,
}
impl JavascriptHeap {
    /// Create the standard bounded tracing configuration.
    pub fn standard(
        cap: usize,
        limits: CollectionLimits,
    ) -> Result<Self, sim_lib_mutation::ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: JavascriptHeapPolicy::Tracing(limits),
        })
    }
    /// Create the explicit retain configuration.
    pub fn retaining(cap: usize) -> Result<Self, sim_lib_mutation::ArenaError> {
        Ok(Self {
            arena: ManagedArena::new(HardCappedRetainPolicy::new(cap)?),
            policy: JavascriptHeapPolicy::Retain,
        })
    }
    /// Allocate cyclic-capable state in the one shared owner.
    pub fn allocate(
        &mut self,
        value: JavascriptManagedObject,
    ) -> Result<ManagedHandle, sim_lib_mutation::ArenaError> {
        self.arena.allocate(value)
    }
    /// Add a strong edge.
    pub fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<(), sim_lib_mutation::ArenaError> {
        self.arena.get_mut(from)?.edges.push(to.id());
        Ok(())
    }
    /// Number of live allocations.
    pub fn live_len(&self) -> usize {
        self.arena.len()
    }
    /// Selected policy.
    pub const fn policy(&self) -> JavascriptHeapPolicy {
        self.policy
    }
    /// Explicit retention gap when collection is disabled.
    pub const fn cycle_leak_gap(&self) -> Option<&'static str> {
        match self.policy {
            JavascriptHeapPolicy::Retain => {
                Some("unreachable JavaScript cycles are retained until teardown")
            }
            JavascriptHeapPolicy::Tracing(_) => None,
        }
    }
    /// Run a synchronous safepoint.
    pub fn collect(&mut self) -> Result<Option<CollectionReceipt>, CollectionError> {
        match self.policy {
            JavascriptHeapPolicy::Tracing(l) => collect(&mut self.arena, l).map(Some),
            JavascriptHeapPolicy::Retain => Ok(None),
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
    fn shared_collector_reclaims_cycles() {
        let mut h = JavascriptHeap::standard(8, limits()).unwrap();
        let a = h.allocate(JavascriptManagedObject::default()).unwrap();
        let b = h.allocate(JavascriptManagedObject::default()).unwrap();
        h.connect(a, b).unwrap();
        h.connect(b, a).unwrap();
        assert_eq!(h.collect().unwrap().unwrap().swept.len(), 2);
    }
    #[test]
    fn retention_gap_is_explicit() {
        assert!(
            JavascriptHeap::retaining(2)
                .unwrap()
                .cycle_leak_gap()
                .unwrap()
                .contains("cycles")
        );
    }
}
