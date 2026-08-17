use sim_lib_mutation::{
    ArenaError, EdgeId, EphemeronMutationError, ManagedHandle, ManagedNode,
    StrongEdgeMutationError, WeakEdgeMutationError,
};

/// Open JavaScript role label carried by the shared managed node.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JavascriptManagedKind {
    /// Ordinary cyclic object.
    #[default]
    Object,
    /// Lexical environment or closure.
    Environment,
    /// Iterator or suspended frame.
    Frame,
    /// Callable identity whose edges include its captured environment.
    Function,
}

/// Compatibility name for JavaScript's role-bearing shared managed node.
pub type JavascriptManagedObject = ManagedNode<JavascriptManagedKind>;

/// Compatibility name for the shared managed heap instantiated for JavaScript.
pub type JavascriptHeap = sim_lib_gc_tracing::ManagedHeap<JavascriptManagedObject>;

/// Compatibility name for the shared heap policy.
pub type JavascriptHeapPolicy = sim_lib_gc_tracing::ManagedHeapPolicy;

/// JavaScript-named graph operations over the shared heap and node.
pub trait JavascriptHeapExt {
    /// Adds a checked strong edge and returns its stable edge identity.
    fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError>;

    /// Adds a checked weak edge and returns its stable edge identity.
    fn connect_weak(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError>;

    /// Adds a checked ephemeron and returns its stable edge identity.
    fn connect_ephemeron(
        &mut self,
        from: ManagedHandle,
        key: ManagedHandle,
        value: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError>;
}

/// A checked JavaScript managed-graph mutation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptManagedMutationError {
    /// The owning allocation handle is stale.
    Arena(ArenaError),
    /// A strong edge could not be admitted.
    Strong(StrongEdgeMutationError),
    /// A weak edge could not be admitted.
    Weak(WeakEdgeMutationError),
    /// An ephemeron could not be admitted.
    Ephemeron(EphemeronMutationError),
}

impl From<ArenaError> for JavascriptManagedMutationError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}

impl JavascriptHeapExt for JavascriptHeap {
    fn connect(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError> {
        self.get_mut(from)?
            .insert_strong(to.id())
            .map_err(JavascriptManagedMutationError::Strong)
    }

    fn connect_weak(
        &mut self,
        from: ManagedHandle,
        to: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError> {
        self.get_mut(from)?
            .insert_weak(to.id())
            .map_err(JavascriptManagedMutationError::Weak)
    }

    fn connect_ephemeron(
        &mut self,
        from: ManagedHandle,
        key: ManagedHandle,
        value: ManagedHandle,
    ) -> Result<EdgeId, JavascriptManagedMutationError> {
        self.get_mut(from)?
            .insert_ephemeron(key.id(), value.id())
            .map_err(JavascriptManagedMutationError::Ephemeron)
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
    fn shared_node_preserves_strong_cycle_behavior() {
        let mut heap = JavascriptHeap::tracing(8, limits()).unwrap();
        let first = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        let second = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        heap.connect(first, second).unwrap();
        heap.connect(second, first).unwrap();
        assert_eq!(
            heap.collect().unwrap().unwrap().swept,
            [first.id(), second.id()]
        );
    }

    #[test]
    fn shared_heap_preserves_explicit_retention_gap() {
        let mut heap = JavascriptHeap::retaining(2).unwrap();
        heap.allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        assert!(heap.cycle_leak_gap().unwrap().contains("cycles"));
        assert_eq!(heap.collect().unwrap(), None);
        assert_eq!(heap.live_len(), 1);
    }

    #[test]
    fn javascript_weak_and_ephemeron_edges_clear_on_shared_collector() {
        let mut heap = JavascriptHeap::tracing(8, limits()).unwrap();
        let owner = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        let weak_target = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        let key = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
            .unwrap();
        let value = heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))
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
