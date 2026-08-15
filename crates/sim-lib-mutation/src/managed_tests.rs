// conformance: bounded managed arena and versioned tracing contract.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    ArenaError, EdgeAllocationError, EdgeAllocator, EdgeId, EdgeKind, EdgeVisitor,
    EphemeronMutationError, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedNode,
    ManagedObject, ManagedRole, StrongEdgeMutationError, TraceSnapshot, WeakEdgeMutationError,
};

#[derive(Clone, Debug)]
enum Edge {
    Strong(ManagedId),
    Weak(Option<ManagedId>),
    Ephemeron { key: ManagedId, value: ManagedId },
}

#[derive(Clone, Debug, Default)]
struct Node(Vec<Edge>);

impl ManagedObject for Node {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (index, edge) in self.0.iter().enumerate() {
            let edge_id = EdgeId(u32::try_from(index).expect("small test graph"));
            match edge {
                Edge::Strong(target) => visitor.strong(edge_id, *target),
                Edge::Weak(Some(target)) => visitor.weak(edge_id, *target),
                Edge::Weak(None) => {}
                Edge::Ephemeron { key, value } => visitor.ephemeron(edge_id, *key, *value),
            }
        }
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        match self.0.get_mut(edge.0 as usize) {
            Some(Edge::Weak(target)) if *target == Some(expected) => target.take().is_some(),
            _ => false,
        }
    }
}

#[test]
fn edge_ids_are_monotonic_typed_and_never_reused() {
    let mut allocator = EdgeAllocator::new();
    let first = allocator.allocate(EdgeKind::Strong).unwrap();
    let removed = first;
    let second = allocator.allocate(EdgeKind::Weak).unwrap();

    assert_eq!(removed.id().allocation_ordinal(), 0);
    assert_eq!(removed.kind(), EdgeKind::Strong);
    assert_eq!(second.id().allocation_ordinal(), 1);
    assert_eq!(second.kind(), EdgeKind::Weak);
    assert!(removed.id() < second.id());
}

#[test]
fn edge_identity_overflow_fails_closed() {
    let mut allocator = EdgeAllocator::starting_at(u32::MAX);
    let last = allocator.allocate(EdgeKind::Ephemeron).unwrap();
    assert_eq!(last.id().allocation_ordinal(), u32::MAX);
    assert_eq!(last.kind(), EdgeKind::Ephemeron);
    assert_eq!(
        allocator.allocate(EdgeKind::Strong),
        Err(EdgeAllocationError::IdentityExhausted)
    );
    assert_eq!(
        allocator.allocate(EdgeKind::Weak),
        Err(EdgeAllocationError::IdentityExhausted)
    );
}

#[test]
fn generic_role_changes_are_graph_neutral() {
    let mut role = ManagedRole::new(String::from("instance"));
    let mut allocator = EdgeAllocator::new();
    let before = [
        allocator.allocate(EdgeKind::Strong).unwrap(),
        allocator.allocate(EdgeKind::Weak).unwrap(),
    ];

    assert_eq!(role.replace_role(String::from("prototype")), "instance");
    assert_eq!(role.role(), "prototype");

    let after = allocator.allocate(EdgeKind::Strong).unwrap();
    assert_eq!(before.map(|edge| edge.id().allocation_ordinal()), [0, 1]);
    assert_eq!(after.id().allocation_ordinal(), 2);
}

#[test]
fn managed_node_strong_graph_matches_ordered_model_across_mutations() {
    let mut arena = arena(5);
    let targets = (0..4)
        .map(|_| arena.allocate(Node::default()).unwrap())
        .collect::<Vec<_>>();
    let mut node = ManagedNode::new(String::from("object"));
    let first = node.insert_strong(targets[0].id()).unwrap();
    let second = node.insert_strong(targets[1].id()).unwrap();
    let third = node.insert_strong(targets[2].id()).unwrap();

    node.replace_strong(second, targets[1].id(), targets[3].id())
        .unwrap();
    assert_eq!(
        node.remove_strong(first, targets[0].id()),
        Ok(targets[0].id())
    );

    let mut traced = Vec::new();
    struct StrongTrace<'a>(&'a mut Vec<(EdgeId, ManagedId)>);
    impl EdgeVisitor for StrongTrace<'_> {
        fn strong(&mut self, edge: EdgeId, target: ManagedId) {
            self.0.push((edge, target));
        }
        fn weak(&mut self, _edge: EdgeId, _target: ManagedId) {}
        fn ephemeron(&mut self, _edge: EdgeId, _key: ManagedId, _value: ManagedId) {}
    }
    node.trace_edges(&mut StrongTrace(&mut traced));

    assert_eq!(
        traced,
        vec![(second, targets[3].id()), (third, targets[2].id())]
    );
    assert_eq!(node.role(), "object");
    assert_eq!(node.replace_role(String::from("array")), "object");
    assert_eq!(node.role(), "array");
    assert!(first < second && second < third);
}

#[test]
fn managed_node_failed_strong_mutations_are_exact_and_atomic() {
    let mut arena = arena(3);
    let original = arena.allocate(Node::default()).unwrap().id();
    let stale_expectation = arena.allocate(Node::default()).unwrap().id();
    let replacement = arena.allocate(Node::default()).unwrap().id();
    let mut node = ManagedNode::new(());
    let edge = node.insert_strong(original).unwrap();
    let before = node.clone();

    assert_eq!(
        node.replace_strong(edge, stale_expectation, replacement),
        Err(StrongEdgeMutationError::TargetChanged {
            expected: stale_expectation,
            actual: original,
        })
    );
    assert_eq!(node, before);
    assert_eq!(
        node.remove_strong(edge, stale_expectation),
        Err(StrongEdgeMutationError::TargetChanged {
            expected: stale_expectation,
            actual: original,
        })
    );
    assert_eq!(node, before);
    assert_eq!(
        node.remove_strong(EdgeId(edge.0 + 1), original),
        Err(StrongEdgeMutationError::UnknownEdge(EdgeId(edge.0 + 1)))
    );
    assert_eq!(node, before);
}

#[test]
fn managed_node_weak_edges_mutate_trace_and_clear_exactly_once() {
    let mut arena = arena(4);
    let first_target = arena.allocate(Node::default()).unwrap().id();
    let second_target = arena.allocate(Node::default()).unwrap().id();
    let mismatch = arena.allocate(Node::default()).unwrap().id();
    let mut node = ManagedNode::new(());
    let removed = node.insert_weak(first_target).unwrap();
    let strong = node.insert_strong(mismatch).unwrap();
    let retained = node.insert_weak(first_target).unwrap();

    node.replace_weak(retained, first_target, second_target)
        .unwrap();
    assert_eq!(node.remove_weak(removed, first_target), Ok(first_target));
    let after_clear = node.insert_weak(first_target).unwrap();

    let before_mismatch = node.clone();
    assert_eq!(
        node.replace_weak(retained, mismatch, first_target),
        Err(WeakEdgeMutationError::TargetChanged {
            expected: mismatch,
            actual: second_target,
        })
    );
    assert_eq!(node, before_mismatch);
    assert!(!node.clear_weak_edge(retained, mismatch));
    assert!(node.clear_weak_edge(retained, second_target));
    assert!(!node.clear_weak_edge(retained, second_target));

    #[derive(Default)]
    struct OrderedTrace(Vec<(EdgeKind, EdgeId, ManagedId)>);
    impl EdgeVisitor for OrderedTrace {
        fn strong(&mut self, edge: EdgeId, target: ManagedId) {
            self.0.push((EdgeKind::Strong, edge, target));
        }
        fn weak(&mut self, edge: EdgeId, target: ManagedId) {
            self.0.push((EdgeKind::Weak, edge, target));
        }
        fn ephemeron(&mut self, _edge: EdgeId, _key: ManagedId, _value: ManagedId) {}
    }
    let mut traced = OrderedTrace::default();
    node.trace_edges(&mut traced);
    assert_eq!(
        traced.0,
        vec![
            (EdgeKind::Strong, strong, mismatch),
            (EdgeKind::Weak, after_clear, first_target),
        ]
    );
    assert!(removed < strong && strong < retained && retained < after_clear);
}

#[test]
fn managed_node_ephemerons_mutate_trace_and_clear_exact_pairs_once() {
    let mut arena = arena(6);
    let key = arena.allocate(Node::default()).unwrap().id();
    let value = arena.allocate(Node::default()).unwrap().id();
    let replacement_key = arena.allocate(Node::default()).unwrap().id();
    let replacement_value = arena.allocate(Node::default()).unwrap().id();
    let mismatch = arena.allocate(Node::default()).unwrap().id();
    let mut node = ManagedNode::new(());
    let removed = node.insert_ephemeron(key, value).unwrap();
    let strong = node.insert_strong(mismatch).unwrap();
    let retained = node.insert_ephemeron(key, value).unwrap();

    node.replace_ephemeron(retained, (key, value), (replacement_key, replacement_value))
        .unwrap();
    assert_eq!(
        node.remove_ephemeron(removed, (key, value)),
        Ok((key, value))
    );
    let after_clear = node.insert_ephemeron(key, value).unwrap();

    let before_mismatch = node.clone();
    assert_eq!(
        node.replace_ephemeron(retained, (mismatch, value), (key, value)),
        Err(EphemeronMutationError::EntryChanged {
            expected_key: mismatch,
            expected_value: value,
            actual_key: replacement_key,
            actual_value: replacement_value,
        })
    );
    assert_eq!(node, before_mismatch);
    assert!(!node.clear_ephemeron_edge(retained, replacement_key, mismatch));
    assert!(node.clear_ephemeron_edge(retained, replacement_key, replacement_value));
    assert!(!node.clear_ephemeron_edge(retained, replacement_key, replacement_value));

    #[derive(Default)]
    struct OrderedTrace(Vec<(EdgeKind, EdgeId, ManagedId, ManagedId)>);
    impl EdgeVisitor for OrderedTrace {
        fn strong(&mut self, edge: EdgeId, target: ManagedId) {
            self.0.push((EdgeKind::Strong, edge, target, target));
        }
        fn weak(&mut self, _edge: EdgeId, _target: ManagedId) {}
        fn ephemeron(&mut self, edge: EdgeId, key: ManagedId, value: ManagedId) {
            self.0.push((EdgeKind::Ephemeron, edge, key, value));
        }
    }
    let mut traced = OrderedTrace::default();
    node.trace_edges(&mut traced);
    assert_eq!(
        traced.0,
        vec![
            (EdgeKind::Strong, strong, mismatch, mismatch),
            (EdgeKind::Ephemeron, after_clear, key, value),
        ]
    );
    assert!(removed < strong && strong < retained && retained < after_clear);
}

#[derive(Default)]
struct CollectedEdges {
    strong: Vec<ManagedId>,
    weak: Vec<(EdgeId, ManagedId)>,
    ephemerons: Vec<(ManagedId, ManagedId)>,
}

impl EdgeVisitor for CollectedEdges {
    fn strong(&mut self, _edge: EdgeId, target: ManagedId) {
        self.strong.push(target);
    }

    fn weak(&mut self, edge: EdgeId, target: ManagedId) {
        self.weak.push((edge, target));
    }

    fn ephemeron(&mut self, _edge: EdgeId, key: ManagedId, value: ManagedId) {
        self.ephemerons.push((key, value));
    }
}

/// Deliberately tiny collector-shaped client used to validate the contract,
/// not production collection policy.
fn reference_trace(snapshot: &TraceSnapshot<'_, Node>) -> (BTreeSet<ManagedId>, usize) {
    let mut marked = BTreeSet::new();
    let mut queue = snapshot.roots().collect::<VecDeque<_>>();
    let mut ephemerons = Vec::new();

    while let Some(id) = queue.pop_front() {
        if !marked.insert(id) {
            continue;
        }
        let mut edges = CollectedEdges::default();
        snapshot.visit_edges(id, &mut edges).unwrap();
        queue.extend(edges.strong);
        ephemerons.extend(edges.ephemerons);
    }

    let mut fixpoint_rounds = 0;
    loop {
        fixpoint_rounds += 1;
        let additions = ephemerons
            .iter()
            .filter(|(key, value)| marked.contains(key) && !marked.contains(value))
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        queue.extend(additions);
        while let Some(id) = queue.pop_front() {
            if !marked.insert(id) {
                continue;
            }
            let mut edges = CollectedEdges::default();
            snapshot.visit_edges(id, &mut edges).unwrap();
            queue.extend(edges.strong);
            ephemerons.extend(edges.ephemerons);
        }
    }
    (marked, fixpoint_rounds)
}

fn arena(cap: usize) -> ManagedArena<Node> {
    ManagedArena::new(HardCappedRetainPolicy::new(cap).unwrap())
}

#[test]
fn allocation_ids_are_schedule_stable_and_cap_failure_is_atomic() {
    let mut first = arena(2);
    let first_a = first.allocate(Node::default()).unwrap();
    let first_b = first.allocate(Node::default()).unwrap();
    assert_eq!(first_a.id().allocation_ordinal(), 0);
    assert_eq!(first_b.id().allocation_ordinal(), 1);
    assert_eq!(
        first.allocate(Node::default()),
        Err(ArenaError::CapacityExceeded { cap: 2 })
    );
    assert_eq!(first.len(), 2);
    assert_eq!(first.get(first_a).unwrap().0.len(), 0);

    let mut replay = arena(2);
    assert_eq!(replay.allocate(Node::default()).unwrap().id(), first_a.id());
    assert_eq!(replay.allocate(Node::default()).unwrap().id(), first_b.id());
}

#[test]
fn roots_churn_and_stale_handles_are_refused() {
    let mut arena = arena(3);
    let handle = arena.allocate(Node::default()).unwrap();
    let weak = handle.downgrade();
    let first_root = arena.root(handle).unwrap();
    let second_root = arena.root(handle).unwrap();

    assert!(matches!(
        arena.remove(handle),
        Err(ArenaError::ObjectRooted(id)) if id == handle.id()
    ));
    arena.release_root(first_root).unwrap();
    assert_eq!(
        arena.release_root(first_root),
        Err(ArenaError::StaleRoot(first_root.root_id()))
    );
    arena.release_root(second_root).unwrap();
    arena.remove(handle).unwrap();
    assert_eq!(
        arena.upgrade(weak),
        Err(ArenaError::StaleHandle(handle.id()))
    );
    assert!(matches!(
        arena.get(handle),
        Err(ArenaError::StaleHandle(id)) if id == handle.id()
    ));
}

#[test]
fn tracing_is_complete_for_cycles_weak_edges_and_ephemeron_fixpoint() {
    let mut arena = arena(7);
    let root = arena.allocate(Node::default()).unwrap();
    let cycle_a = arena.allocate(Node::default()).unwrap();
    let cycle_b = arena.allocate(Node::default()).unwrap();
    let eph_key = arena.allocate(Node::default()).unwrap();
    let eph_mid = arena.allocate(Node::default()).unwrap();
    let eph_value = arena.allocate(Node::default()).unwrap();
    let dead = arena.allocate(Node::default()).unwrap();

    arena.get_mut(root).unwrap().0 = vec![
        Edge::Strong(cycle_a.id()),
        Edge::Strong(eph_key.id()),
        Edge::Weak(Some(dead.id())),
        Edge::Ephemeron {
            key: eph_mid.id(),
            value: eph_value.id(),
        },
        Edge::Ephemeron {
            key: eph_key.id(),
            value: eph_mid.id(),
        },
    ];
    arena.get_mut(cycle_a).unwrap().0 = vec![Edge::Strong(cycle_b.id())];
    arena.get_mut(cycle_b).unwrap().0 = vec![Edge::Strong(cycle_a.id())];
    let rooted = arena.root(root).unwrap();

    let ((marked, rounds), receipt) = arena.safepoint(reference_trace).unwrap();
    assert_eq!(receipt.sequence, 0);
    assert_eq!(receipt.roots, vec![root.id()]);
    assert_eq!(receipt.objects.len(), 7);
    assert_eq!(rounds, 3);
    assert_eq!(
        marked,
        [root, cycle_a, cycle_b, eph_key, eph_mid, eph_value]
            .map(|handle| handle.id())
            .into_iter()
            .collect()
    );
    assert!(!marked.contains(&dead.id()));

    assert!(
        arena
            .clear_weak_edge(root, EdgeId(2), dead.downgrade())
            .unwrap()
    );
    assert!(
        !arena
            .clear_weak_edge(root, EdgeId(2), dead.downgrade())
            .unwrap()
    );
    let dead_handle = arena.handle(dead.id()).unwrap();
    arena.remove(dead_handle).unwrap();
    assert_eq!(
        arena.handle(dead.id()),
        Err(ArenaError::StaleHandle(dead.id()))
    );
    arena.release_root(rooted).unwrap();
}

#[test]
fn trace_visitation_covers_every_declared_edge_once() {
    let mut arena = arena(4);
    let owner = arena.allocate(Node::default()).unwrap();
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    let c = arena.allocate(Node::default()).unwrap();
    arena.get_mut(owner).unwrap().0 = vec![
        Edge::Strong(a.id()),
        Edge::Weak(Some(b.id())),
        Edge::Ephemeron {
            key: b.id(),
            value: c.id(),
        },
    ];
    arena.root(owner).unwrap();

    arena
        .safepoint(|snapshot| {
            let mut edges = CollectedEdges::default();
            snapshot.visit_edges(owner.id(), &mut edges).unwrap();
            assert_eq!(edges.strong, vec![a.id()]);
            assert_eq!(edges.weak, vec![(EdgeId(1), b.id())]);
            assert_eq!(edges.ephemerons, vec![(b.id(), c.id())]);
        })
        .unwrap();
}

#[test]
fn teardown_receipts_are_allocation_deterministic() {
    fn specimen() -> (Vec<ManagedId>, Vec<crate::RootId>) {
        let mut arena = arena(3);
        let a = arena.allocate(Node::default()).unwrap();
        let b = arena.allocate(Node::default()).unwrap();
        let c = arena.allocate(Node::default()).unwrap();
        arena.root(c).unwrap();
        arena.root(a).unwrap();
        arena.remove(b).unwrap();
        let receipt = arena.teardown();
        assert!(arena.is_empty());
        (receipt.objects, receipt.roots)
    }
    assert_eq!(specimen(), specimen());
    assert_eq!(specimen().0.len(), 2);
}

#[test]
fn root_enumeration_is_registration_order_not_object_order() {
    let mut arena = arena(2);
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    arena.root(b).unwrap();
    arena.root(a).unwrap();
    let (roots, _) = arena
        .safepoint(|snapshot| snapshot.roots().collect::<Vec<_>>())
        .unwrap();
    assert_eq!(roots, vec![b.id(), a.id()]);
}

#[test]
fn object_inventory_is_allocation_order_even_after_removal() {
    let mut arena = arena(3);
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    let c = arena.allocate(Node::default()).unwrap();
    arena.remove(b).unwrap();
    let (objects, _) = arena
        .safepoint(|snapshot| snapshot.objects().collect::<Vec<_>>())
        .unwrap();
    assert_eq!(objects, vec![a.id(), c.id()]);
}

#[test]
fn epoch_guarded_sweep_validates_every_slot_before_mutation() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(4).unwrap());
    let live = arena.allocate(Node::default()).unwrap();
    let garbage = arena.allocate(Node::default()).unwrap();
    let epoch = arena.mutation_epoch();
    let rooted = arena.root(live).unwrap();

    assert!(matches!(
        arena.sweep_at_epoch(epoch, &[garbage.id()]),
        Err(ArenaError::MutationEpochChanged { .. })
    ));
    assert_eq!(arena.len(), 2);

    let current = arena.mutation_epoch();
    assert_eq!(
        arena.sweep_at_epoch(current, &[garbage.id()]).unwrap(),
        vec![garbage.id()]
    );
    assert_eq!(arena.len(), 1);
    assert_eq!(arena.release_root(rooted).unwrap(), live);
}

#[test]
fn invalid_policy_is_closed() {
    assert_eq!(HardCappedRetainPolicy::new(0), Err(ArenaError::InvalidCap));
    let policy = HardCappedRetainPolicy::new(9).unwrap();
    assert_eq!(policy.max_objects(), 9);
}

#[test]
fn collector_client_can_inventory_edges_without_language_types() {
    let mut arena = arena(2);
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    arena.get_mut(a).unwrap().0.push(Edge::Strong(b.id()));
    arena.root(a).unwrap();
    let (inventory, _) = arena
        .safepoint(|snapshot| {
            let mut inventory = BTreeMap::new();
            for object in snapshot.objects() {
                let mut edges = CollectedEdges::default();
                snapshot.visit_edges(object, &mut edges).unwrap();
                inventory.insert(object, edges.strong);
            }
            inventory
        })
        .unwrap();
    assert_eq!(inventory.get(&a.id()), Some(&vec![b.id()]));
}
