// conformance: bounded stop-the-world tracing collection.

use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedObject,
};

use crate::{CollectionError, CollectionLimits, LimitKind, collect};

#[derive(Clone, Default)]
struct Node {
    strong: Vec<ManagedId>,
}
impl ManagedObject for Node {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (edge, target) in self.strong.iter().copied().enumerate() {
            visitor.strong(EdgeId(edge as u32), target);
        }
    }
    fn clear_weak_edge(&mut self, _: EdgeId, _: ManagedId) -> bool {
        false
    }
}

fn limits() -> CollectionLimits {
    CollectionLimits {
        objects: 20_000,
        edges: 40_000,
        stack: 20_000,
        work: 100_000,
    }
}

#[test]
fn roots_shared_subgraphs_and_unreachable_cycles_are_collected() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(8).unwrap());
    let root = arena.allocate(Node::default()).unwrap();
    let left = arena.allocate(Node::default()).unwrap();
    let right = arena.allocate(Node::default()).unwrap();
    let shared = arena.allocate(Node::default()).unwrap();
    let cycle_a = arena.allocate(Node::default()).unwrap();
    let cycle_b = arena.allocate(Node::default()).unwrap();
    arena.get_mut(root).unwrap().strong = vec![left.id(), right.id()];
    arena.get_mut(left).unwrap().strong = vec![shared.id()];
    arena.get_mut(right).unwrap().strong = vec![shared.id()];
    arena.get_mut(cycle_a).unwrap().strong = vec![cycle_b.id()];
    arena.get_mut(cycle_b).unwrap().strong = vec![cycle_a.id()];
    let _rooted = arena.root(root).unwrap();

    let receipt = collect(&mut arena, limits()).unwrap();
    assert_eq!(
        receipt.marked,
        vec![root.id(), left.id(), right.id(), shared.id()]
    );
    assert_eq!(receipt.swept, vec![cycle_a.id(), cycle_b.id()]);
    assert_eq!(arena.len(), 4);
    assert!(arena.handle(cycle_a.id()).is_err());
}

#[test]
fn deep_graph_uses_the_bounded_iterative_stack() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(12_000).unwrap());
    let mut nodes = Vec::new();
    for _ in 0..12_000 {
        nodes.push(arena.allocate(Node::default()).unwrap());
    }
    for pair in nodes.windows(2) {
        arena.get_mut(pair[0]).unwrap().strong.push(pair[1].id());
    }
    let _rooted = arena.root(nodes[0]).unwrap();
    let receipt = collect(&mut arena, limits()).unwrap();
    assert_eq!(receipt.marked.len(), 12_000);
    assert!(receipt.swept.is_empty());
}

#[test]
fn admission_failure_is_repeatable_and_leaves_graph_untouched() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(3).unwrap());
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    arena.get_mut(a).unwrap().strong.push(b.id());
    let rooted = arena.root(a).unwrap();
    let before_epoch = arena.mutation_epoch();
    let tight = CollectionLimits {
        objects: 3,
        edges: 0,
        stack: 3,
        work: 10,
    };
    let first = collect(&mut arena, tight).unwrap_err();
    let second = collect(&mut arena, tight).unwrap_err();
    assert_eq!(first, second);
    assert!(matches!(first, CollectionError::Limit(ref r) if r.kind == LimitKind::Edges));
    assert_eq!(arena.mutation_epoch(), before_epoch);
    assert_eq!(arena.len(), 2);
    assert_eq!(arena.release_root(rooted).unwrap(), a);
}

#[test]
fn root_churn_and_identical_schedules_produce_identical_receipts() {
    fn specimen() -> (crate::CollectionReceipt, usize) {
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(5).unwrap());
        let a = arena.allocate(Node::default()).unwrap();
        let b = arena.allocate(Node::default()).unwrap();
        let stale = arena.allocate(Node::default()).unwrap();
        arena.get_mut(a).unwrap().strong.push(b.id());
        let transient = arena.root(stale).unwrap();
        arena.release_root(transient).unwrap();
        let _rooted = arena.root(a).unwrap();
        let receipt = collect(&mut arena, limits()).unwrap();
        assert!(arena.handle(stale.id()).is_err());
        (receipt, arena.len())
    }
    assert_eq!(specimen(), specimen());
}

#[test]
fn randomized_graphs_match_a_non_language_reference_model() {
    for seed in 1_u64..40 {
        let mut state = seed;
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(48).unwrap());
        let nodes = (0..48)
            .map(|_| arena.allocate(Node::default()).unwrap())
            .collect::<Vec<_>>();
        let mut model = vec![Vec::<usize>::new(); 48];
        for (owner, edges) in model.iter_mut().enumerate() {
            for _ in 0..3 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let target = (state as usize) % 48;
                edges.push(target);
                arena
                    .get_mut(nodes[owner])
                    .unwrap()
                    .strong
                    .push(nodes[target].id());
            }
        }
        let roots = [0, (seed as usize) % 48];
        for root in roots {
            let _ = arena.root(nodes[root]).unwrap();
        }
        let mut expected = std::collections::BTreeSet::new();
        let mut pending = roots.to_vec();
        while let Some(node) = pending.pop() {
            if expected.insert(node) {
                pending.extend(model[node].iter().copied());
            }
        }
        let receipt = collect(&mut arena, limits()).unwrap();
        let actual = receipt
            .marked
            .iter()
            .map(|id| id.allocation_ordinal() as usize)
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected.into_iter().collect::<Vec<_>>(),
            "seed {seed}"
        );
    }
}
