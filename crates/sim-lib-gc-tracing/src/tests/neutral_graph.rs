use std::collections::BTreeSet;

use sim_lib_mutation::{EdgeId, EdgeVisitor, ManagedArena, ManagedId, ManagedObject};

use super::{CollectionError, CollectionLimits, HardCappedRetainPolicy, LimitKind, collect};
use crate::{CollectionReceipt, ManagedHeap, ManagedHeapPolicy};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct NeutralNode {
    strong: Vec<ManagedId>,
    weak: Vec<Option<ManagedId>>,
    ephemerons: Vec<Option<(ManagedId, ManagedId)>>,
}

impl ManagedObject for NeutralNode {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (edge, target) in self.strong.iter().copied().enumerate() {
            visitor.strong(EdgeId(edge as u32), target);
        }
        let weak_offset = self.strong.len();
        for (edge, target) in self.weak.iter().enumerate() {
            if let Some(target) = target {
                visitor.weak(EdgeId((weak_offset + edge) as u32), *target);
            }
        }
        let ephemeron_offset = weak_offset + self.weak.len();
        for (edge, entry) in self.ephemerons.iter().enumerate() {
            if let Some((key, value)) = entry {
                visitor.ephemeron(EdgeId((ephemeron_offset + edge) as u32), *key, *value);
            }
        }
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        let Some(index) = (edge.0 as usize).checked_sub(self.strong.len()) else {
            return false;
        };
        self.weak.get_mut(index).is_some_and(|entry| {
            if *entry == Some(expected) {
                *entry = None;
                true
            } else {
                false
            }
        })
    }

    fn clear_ephemeron_edge(
        &mut self,
        edge: EdgeId,
        expected_key: ManagedId,
        expected_value: ManagedId,
    ) -> bool {
        let Some(index) = (edge.0 as usize).checked_sub(self.strong.len() + self.weak.len()) else {
            return false;
        };
        self.ephemerons.get_mut(index).is_some_and(|entry| {
            if *entry == Some((expected_key, expected_value)) {
                *entry = None;
                true
            } else {
                false
            }
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ModelReceipt {
    live: Vec<ManagedId>,
    swept: Vec<ManagedId>,
    cleared_weak: Vec<(ManagedId, EdgeId)>,
    cleared_ephemerons: Vec<(ManagedId, EdgeId)>,
}

fn reference_trace(
    nodes: &[(ManagedId, NeutralNode)],
    roots: &[ManagedId],
    kept_alive: &[ManagedId],
) -> ModelReceipt {
    let mut live = BTreeSet::new();
    let mut pending = roots.iter().chain(kept_alive).copied().collect::<Vec<_>>();
    loop {
        while let Some(id) = pending.pop() {
            if !live.insert(id) {
                continue;
            }
            let node = &nodes[id.allocation_ordinal() as usize].1;
            pending.extend(node.strong.iter().copied());
        }
        let mut changed = false;
        for (_, node) in nodes {
            for (key, value) in node.ephemerons.iter().flatten() {
                if live.contains(key) && !live.contains(value) {
                    pending.push(*value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let swept = nodes
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !live.contains(id))
        .collect();
    let mut cleared_weak = Vec::new();
    let mut cleared_ephemerons = Vec::new();
    for (owner, node) in nodes.iter().filter(|(id, _)| live.contains(id)) {
        for (index, target) in node.weak.iter().enumerate() {
            if target.is_some_and(|target| !live.contains(&target)) {
                cleared_weak.push((*owner, EdgeId((node.strong.len() + index) as u32)));
            }
        }
        for (index, entry) in node.ephemerons.iter().enumerate() {
            if entry.is_some_and(|(key, _)| !live.contains(&key)) {
                cleared_ephemerons.push((
                    *owner,
                    EdgeId((node.strong.len() + node.weak.len() + index) as u32),
                ));
            }
        }
    }
    ModelReceipt {
        live: live.into_iter().collect(),
        swept,
        cleared_weak,
        cleared_ephemerons,
    }
}

fn generous_limits() -> CollectionLimits {
    CollectionLimits {
        objects: 16,
        edges: 32,
        stack: 16,
        work: 256,
        clears: 16,
        finalizers: 0,
    }
}

fn mixed_specimen() -> (CollectionReceipt, CollectionReceipt) {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(10).unwrap());
    let handles = (0..10)
        .map(|_| arena.allocate(NeutralNode::default()).unwrap())
        .collect::<Vec<_>>();
    let ids = handles.iter().map(|handle| handle.id()).collect::<Vec<_>>();

    arena.get_mut(handles[0]).unwrap().strong.push(ids[1]);
    arena.get_mut(handles[0]).unwrap().weak.push(Some(ids[6]));
    arena.get_mut(handles[0]).unwrap().ephemerons =
        vec![Some((ids[1], ids[2])), Some((ids[5], ids[7]))];
    arena.get_mut(handles[1]).unwrap().strong.push(ids[0]);
    arena
        .get_mut(handles[1])
        .unwrap()
        .ephemerons
        .push(Some((ids[3], ids[4])));
    arena.get_mut(handles[2]).unwrap().strong.push(ids[3]);
    arena.get_mut(handles[5]).unwrap().strong.push(ids[6]);
    arena.get_mut(handles[6]).unwrap().strong.push(ids[5]);
    arena.get_mut(handles[8]).unwrap().strong.push(ids[9]);
    let rooted = arena.root(handles[0]).unwrap();
    arena.upgrade(handles[8].downgrade()).unwrap();

    let snapshot = handles
        .iter()
        .map(|handle| (handle.id(), arena.get(*handle).unwrap().clone()))
        .collect::<Vec<_>>();
    let expected = reference_trace(&snapshot, &[ids[0]], &[ids[8]]);
    let before_epoch = arena.mutation_epoch();
    let tight = CollectionLimits {
        edges: 0,
        ..generous_limits()
    };
    let first_error = collect(&mut arena, tight).unwrap_err();
    let second_error = collect(&mut arena, tight).unwrap_err();
    assert_eq!(first_error, second_error);
    assert!(
        matches!(first_error, CollectionError::Limit(ref receipt) if receipt.kind == LimitKind::Edges)
    );
    assert_eq!(arena.mutation_epoch(), before_epoch);
    assert_eq!(
        handles
            .iter()
            .map(|handle| (handle.id(), arena.get(*handle).unwrap().clone()))
            .collect::<Vec<_>>(),
        snapshot,
        "a refused plan must not sweep or clear the graph",
    );

    let first = collect(&mut arena, generous_limits()).unwrap();
    assert_eq!(first.marked, expected.live);
    assert_eq!(first.swept, expected.swept);
    assert_eq!(first.cleared_weak, expected.cleared_weak);
    assert_eq!(first.cleared_ephemerons, expected.cleared_ephemerons);

    arena.release_root(rooted).unwrap();
    let survivors = handles
        .iter()
        .filter_map(|handle| {
            arena
                .get(*handle)
                .ok()
                .map(|node| (handle.id(), node.clone()))
        })
        .collect::<Vec<_>>();
    let expected = reference_trace(&survivors, &[], &[]);
    let second = collect(&mut arena, generous_limits()).unwrap();
    assert_eq!(second.marked, expected.live);
    assert_eq!(second.swept, expected.swept);
    assert_eq!(second.cleared_weak, expected.cleared_weak);
    assert_eq!(second.cleared_ephemerons, expected.cleared_ephemerons);
    assert!(arena.is_empty());
    assert_eq!(arena.teardown().objects, []);
    (first, second)
}

#[test]
fn neutral_mixed_graph_matches_reference_across_root_and_kept_alive_epochs() {
    assert_eq!(mixed_specimen(), mixed_specimen());
}

#[test]
fn ephemeron_keyed_cache_reclaims_values_and_their_captured_graphs() {
    const LOAD_DROP_CYCLES: usize = 32;

    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(4).unwrap());
    let cache = arena.allocate(NeutralNode::default()).unwrap();
    let cache_root = arena.root(cache).unwrap();

    for cycle in 0..LOAD_DROP_CYCLES {
        let key = arena.allocate(NeutralNode::default()).unwrap();
        let value = arena.allocate(NeutralNode::default()).unwrap();
        let captured = arena.allocate(NeutralNode::default()).unwrap();
        arena.get_mut(value).unwrap().strong.push(captured.id());
        arena
            .get_mut(cache)
            .unwrap()
            .ephemerons
            .push(Some((key.id(), value.id())));
        let key_root = arena.root(key).unwrap();

        let retained = collect(&mut arena, generous_limits()).unwrap();
        assert_eq!(
            retained.marked,
            [cache.id(), key.id(), value.id(), captured.id()],
            "cycle {cycle}: a live cache key must retain its value and captures",
        );
        assert!(retained.swept.is_empty());
        assert!(retained.cleared_ephemerons.is_empty());
        assert_eq!(arena.len(), 4);

        arena.release_root(key_root).unwrap();
        let reclaimed = collect(&mut arena, generous_limits()).unwrap();
        assert_eq!(reclaimed.marked, [cache.id()]);
        assert_eq!(
            reclaimed.swept,
            [key.id(), value.id(), captured.id()],
            "cycle {cycle}: the value graph must not become a strong-map fallback",
        );
        assert_eq!(
            reclaimed.cleared_ephemerons,
            [(cache.id(), EdgeId(cycle as u32))],
            "cycle {cycle}: clearing must identify the exact cache entry",
        );
        assert!(reclaimed.cleared_weak.is_empty());
        assert_eq!(
            arena.len(),
            1,
            "cycle {cycle}: live storage must stay bounded"
        );
        assert!(arena.handle(key.id()).is_err());
        assert!(arena.handle(value.id()).is_err());
        assert!(arena.handle(captured.id()).is_err());

        let settled = collect(&mut arena, generous_limits()).unwrap();
        assert!(settled.swept.is_empty());
        assert!(settled.cleared_ephemerons.is_empty());
        assert_eq!(arena.len(), 1);
    }

    arena.release_root(cache_root).unwrap();
    let closed = collect(&mut arena, generous_limits()).unwrap();
    assert_eq!(closed.swept, [cache.id()]);
    assert!(arena.is_empty());
}

#[test]
fn retaining_mode_defers_cycles_to_deterministic_teardown() {
    let mut heap = ManagedHeap::retaining(2).unwrap();
    let first = heap.allocate(NeutralNode::default()).unwrap();
    let second = heap.allocate(NeutralNode::default()).unwrap();
    heap.get_mut(first).unwrap().strong.push(second.id());
    heap.get_mut(second).unwrap().strong.push(first.id());
    assert_eq!(heap.policy(), ManagedHeapPolicy::Retain);
    assert_eq!(heap.collect().unwrap(), None);
    assert_eq!(heap.live_len(), 2);
    let receipt = heap.teardown();
    assert_eq!(receipt.objects, [first.id(), second.id()]);
    assert!(receipt.roots.is_empty());
    assert_eq!(heap.live_len(), 0);
}
