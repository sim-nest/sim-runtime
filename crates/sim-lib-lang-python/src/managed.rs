use sim_lib_mutation::{
    ArenaError, EdgeId, EphemeronMutationError, ManagedHandle, ManagedNode,
    StrongEdgeMutationError, WeakEdgeMutationError,
};

/// Open Python role label carried by the shared managed node.
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

/// Compatibility name for Python's role-bearing shared managed node.
pub type PythonManagedObject = ManagedNode<PythonManagedKind>;

/// Compatibility name for the shared managed heap instantiated for Python.
pub type PythonHeap = sim_lib_gc_tracing::ManagedHeap<PythonManagedObject>;

/// Compatibility name for the shared heap policy.
pub type PythonHeapPolicy = sim_lib_gc_tracing::ManagedHeapPolicy;

/// Python-named graph operations over the shared heap and node.
pub trait PythonHeapExt {
    /// Adds a checked strong edge and returns its stable edge identity.
    fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError>;

    /// Adds a checked weak edge and returns its stable edge identity.
    fn connect_weak(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError>;

    /// Adds a checked ephemeron and returns its stable edge identity.
    fn connect_ephemeron(
        &mut self,
        from: ManagedHandle,
        key: ManagedHandle,
        value: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError>;
}

/// A checked Python managed-graph mutation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PythonManagedMutationError {
    /// The owning allocation handle is stale.
    Arena(ArenaError),
    /// A strong edge could not be admitted.
    Strong(StrongEdgeMutationError),
    /// A weak edge could not be admitted.
    Weak(WeakEdgeMutationError),
    /// An ephemeron could not be admitted.
    Ephemeron(EphemeronMutationError),
}

impl From<ArenaError> for PythonManagedMutationError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}

impl PythonHeapExt for PythonHeap {
    fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError> {
        self.get_mut(from)?
            .insert_strong(to.id())
            .map_err(PythonManagedMutationError::Strong)
    }

    fn connect_weak(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError> {
        self.get_mut(from)?
            .insert_weak(to.id())
            .map_err(PythonManagedMutationError::Weak)
    }

    fn connect_ephemeron(
        &mut self,
        from: ManagedHandle,
        key: ManagedHandle,
        value: ManagedHandle,
    ) -> Result<EdgeId, PythonManagedMutationError> {
        self.get_mut(from)?
            .insert_ephemeron(key.id(), value.id())
            .map_err(PythonManagedMutationError::Ephemeron)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_lib_gc_tracing::CollectionLimits;

    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 8,
            edges: 16,
            stack: 8,
            work: 64,
            clears: 8,
            finalizers: 0,
        }
    }

    #[test]
    fn shared_node_preserves_heterogeneous_cycle_behavior() {
        let mut heap = PythonHeap::tracing(8, limits()).unwrap();
        let kinds = [
            PythonManagedKind::Instance,
            PythonManagedKind::Closure,
            PythonManagedKind::Frame,
            PythonManagedKind::Exception,
            PythonManagedKind::Container,
        ];
        let handles: Vec<_> = kinds
            .into_iter()
            .map(|kind| heap.allocate(PythonManagedObject::new(kind)).unwrap())
            .collect();
        for pair in handles.windows(2) {
            heap.connect(pair[0], pair[1]).unwrap();
        }
        heap.connect(handles[4], handles[0]).unwrap();
        let visible_result = 42;
        assert_eq!(
            heap.collect().unwrap().unwrap().swept,
            handles.iter().map(|handle| handle.id()).collect::<Vec<_>>()
        );
        assert_eq!(heap.live_len(), 0);
        assert_eq!(visible_result, 42);
    }

    #[test]
    fn shared_heap_preserves_exact_retention_gap() {
        let mut heap = PythonHeap::retaining(2).unwrap();
        heap.allocate(PythonManagedObject::new(PythonManagedKind::Instance))
            .unwrap();
        assert_eq!(
            heap.cycle_leak_gap(),
            Some("unreachable strong cycles are retained until heap teardown")
        );
        assert_eq!(heap.collect().unwrap(), None);
        assert_eq!(heap.live_len(), 1);
    }

    #[test]
    fn python_weak_and_ephemeron_edges_clear_on_shared_collector() {
        let mut heap = PythonHeap::tracing(8, limits()).unwrap();
        let owner = heap
            .allocate(PythonManagedObject::new(PythonManagedKind::Container))
            .unwrap();
        let weak_target = heap
            .allocate(PythonManagedObject::new(PythonManagedKind::Instance))
            .unwrap();
        let key = heap
            .allocate(PythonManagedObject::new(PythonManagedKind::Instance))
            .unwrap();
        let value = heap
            .allocate(PythonManagedObject::new(PythonManagedKind::Instance))
            .unwrap();
        let weak = heap.connect_weak(owner, weak_target).unwrap();
        let ephemeron = heap.connect_ephemeron(owner, key, value).unwrap();
        let root = heap.root(owner).unwrap();

        let receipt = heap.collect().unwrap().unwrap();
        assert_eq!(receipt.swept, [weak_target.id(), key.id(), value.id()]);
        assert_eq!(receipt.cleared_weak, [(owner.id(), weak)]);
        assert_eq!(receipt.cleared_ephemerons, [(owner.id(), ephemeron)]);
        assert!(heap.get(owner).unwrap().edge_snapshot().is_empty());
        heap.release_root(root).unwrap();
    }
}
