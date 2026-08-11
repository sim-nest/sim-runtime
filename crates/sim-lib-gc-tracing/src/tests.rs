// conformance: bounded stop-the-world tracing collection.

use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedObject,
};

use std::sync::{Arc, Mutex};

use sim_lib_control::{AdmissionLimit, JobQueues, RuntimeJobClass, WorkLimit};

use crate::{
    CollectionError, CollectionLimits, CorrectnessDimension, FinalizationRegistry, LimitKind,
    collect, collect_with_finalization,
};

#[derive(Clone, Default)]
struct Node {
    strong: Vec<ManagedId>,
    weak: Vec<Option<ManagedId>>,
    ephemerons: Vec<Option<(ManagedId, ManagedId)>>,
}
impl ManagedObject for Node {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (edge, target) in self.strong.iter().copied().enumerate() {
            visitor.strong(EdgeId(edge as u32), target);
        }
        let offset = self.strong.len();
        for (edge, target) in self.weak.iter().enumerate() {
            if let Some(target) = target {
                visitor.weak(EdgeId((offset + edge) as u32), *target);
            }
        }
        let offset = offset + self.weak.len();
        for (edge, entry) in self.ephemerons.iter().enumerate() {
            if let Some((key, value)) = entry {
                visitor.ephemeron(EdgeId((offset + edge) as u32), *key, *value);
            }
        }
    }
    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        let index = edge.0 as usize - self.strong.len();
        self.weak.get_mut(index).is_some_and(|entry| {
            if *entry == Some(expected) {
                *entry = None;
                true
            } else {
                false
            }
        })
    }
    fn clear_ephemeron_edge(&mut self, edge: EdgeId, key: ManagedId, value: ManagedId) -> bool {
        let index = edge.0 as usize - self.strong.len() - self.weak.len();
        self.ephemerons.get_mut(index).is_some_and(|entry| {
            if *entry == Some((key, value)) {
                *entry = None;
                true
            } else {
                false
            }
        })
    }
}

#[test]
fn ephemeron_fixpoint_and_kept_alive_epoch_obey_strong_liveness() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(7).unwrap());
    let root = arena.allocate(Node::default()).unwrap();
    let key1 = arena.allocate(Node::default()).unwrap();
    let value1 = arena.allocate(Node::default()).unwrap();
    let key2 = arena.allocate(Node::default()).unwrap();
    let value2 = arena.allocate(Node::default()).unwrap();
    let dead_key = arena.allocate(Node::default()).unwrap();
    let dead_value = arena.allocate(Node::default()).unwrap();
    arena.get_mut(root).unwrap().strong.push(key1.id());
    arena.get_mut(root).unwrap().ephemerons = vec![
        Some((key1.id(), value1.id())),
        Some((dead_key.id(), dead_value.id())),
    ];
    arena.get_mut(value1).unwrap().strong.push(key2.id());
    arena
        .get_mut(value1)
        .unwrap()
        .ephemerons
        .push(Some((key2.id(), value2.id())));
    let _ = arena.root(root).unwrap();
    assert_eq!(arena.upgrade(dead_value.downgrade()).unwrap(), dead_value);

    let first = collect(&mut arena, limits()).unwrap();
    assert!(first.marked.contains(&value2.id()));
    assert!(first.marked.contains(&dead_value.id()));
    assert_eq!(first.cleared_ephemerons, vec![(root.id(), EdgeId(2))]);
    let second = collect(&mut arena, limits()).unwrap();
    assert!(second.swept.contains(&dead_value.id()));
    assert!(second.cleared_ephemerons.is_empty());

    let mut quiet = ManagedArena::new(HardCappedRetainPolicy::new(1).unwrap());
    let temporary = quiet.allocate(Node::default()).unwrap();
    quiet.upgrade(temporary.downgrade()).unwrap();
    assert!(collect(&mut quiet, limits()).unwrap().swept.is_empty());
    assert_eq!(
        collect(&mut quiet, limits()).unwrap().swept,
        vec![temporary.id()]
    );
}

#[test]
fn weak_clearing_finalization_and_resurrection_are_bounded_and_at_most_once() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(3).unwrap());
    let owner = arena.allocate(Node::default()).unwrap();
    let dead = arena.allocate(Node::default()).unwrap();
    arena.get_mut(owner).unwrap().weak.push(Some(dead.id()));
    let _ = arena.root(owner).unwrap();
    let mut registry = FinalizationRegistry::default();
    let registration = registry.register(dead.id());
    let mut exhausted = JobQueues::new(AdmissionLimit(0));
    let before = arena.mutation_epoch();
    let error =
        collect_with_finalization(&mut arena, limits(), &mut registry, &mut exhausted, |_| {})
            .unwrap_err();
    assert!(matches!(error, CollectionError::Limit(ref r) if r.kind == LimitKind::Finalizers));
    assert_eq!(arena.mutation_epoch(), before);
    assert_eq!(arena.get(owner).unwrap().weak, vec![Some(dead.id())]);

    let resurrected = Arc::new(Mutex::new(Vec::new()));
    let callback_resurrected = resurrected.clone();
    let mut jobs = JobQueues::new(AdmissionLimit(2));
    let receipt = collect_with_finalization(
        &mut arena,
        limits(),
        &mut registry,
        &mut jobs,
        move |record| {
            callback_resurrected
                .lock()
                .unwrap()
                .push(record.registration)
        },
    )
    .unwrap();
    assert_eq!(receipt.cleared_weak, vec![(owner.id(), EdgeId(0))]);
    assert_eq!(receipt.finalization[0].registration, registration);
    assert!(
        resurrected.lock().unwrap().is_empty(),
        "user code ran during collection"
    );
    assert!(arena.handle(dead.id()).is_err());
    assert_eq!(
        jobs.drain(RuntimeJobClass::Finalization, WorkLimit(1))
            .completed
            .len(),
        1
    );
    assert_eq!(*resurrected.lock().unwrap(), vec![registration]);
    assert!(
        collect_with_finalization(&mut arena, limits(), &mut registry, &mut jobs, |_| {})
            .unwrap()
            .finalization
            .is_empty()
    );
}

#[test]
fn cancelled_finalization_never_enters_the_queue() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(1).unwrap());
    let dead = arena.allocate(Node::default()).unwrap();
    let mut registry = FinalizationRegistry::default();
    let registration = registry.register(dead.id());
    assert!(registry.cancel(registration));
    assert!(!registry.cancel(registration));
    let mut jobs = JobQueues::new(AdmissionLimit(1));
    let receipt = collect_with_finalization(&mut arena, limits(), &mut registry, &mut jobs, |_| {
        panic!("cancelled finalizer ran")
    })
    .unwrap();
    assert!(receipt.finalization.is_empty());
    assert!(
        jobs.drain(RuntimeJobClass::Finalization, WorkLimit(1))
            .completed
            .is_empty()
    );
}

fn limits() -> CollectionLimits {
    CollectionLimits {
        objects: 20_000,
        edges: 40_000,
        stack: 20_000,
        work: 100_000,
        clears: 40_000,
        finalizers: 20_000,
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
        clears: 3,
        finalizers: 3,
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

#[test]
fn correctness_dimensions_are_explicit_and_frozen() {
    assert_eq!(
        CorrectnessDimension::ALL,
        [
            CorrectnessDimension::Safety,
            CorrectnessDimension::Reclamation,
            CorrectnessDimension::WeakAndEphemeron,
            CorrectnessDimension::Finalization,
            CorrectnessDimension::SameScheduleDeterminism,
            CorrectnessDimension::ScheduleIndependenceSafety,
            CorrectnessDimension::BoundedWork,
            CorrectnessDimension::FailureAtomicity,
            CorrectnessDimension::WasmClosure,
        ]
    );
}

#[test]
fn generated_graphs_and_legal_safepoint_schedules_match_model_tracer() {
    for seed in 1_u64..24 {
        for schedule in 0..4 {
            let mut state = seed;
            let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(24).unwrap());
            let nodes = (0..24)
                .map(|_| arena.allocate(Node::default()).unwrap())
                .collect::<Vec<_>>();
            let mut graph = vec![Vec::new(); nodes.len()];
            for (owner, outgoing) in graph.iter_mut().enumerate() {
                for _ in 0..2 {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let target = state as usize % nodes.len();
                    outgoing.push(target);
                    arena
                        .get_mut(nodes[owner])
                        .unwrap()
                        .strong
                        .push(nodes[target].id());
                }
            }
            let root_indexes = [0, (seed as usize + schedule) % nodes.len()];
            let roots = root_indexes
                .into_iter()
                .map(|index| arena.root(nodes[index]).unwrap())
                .collect::<Vec<_>>();
            if schedule & 1 != 0 {
                let transient = arena.root(nodes[23]).unwrap();
                arena.release_root(transient).unwrap();
            }
            if schedule & 2 != 0 {
                arena.upgrade(nodes[22].downgrade()).unwrap();
            }

            let mut expected = std::collections::BTreeSet::new();
            let mut pending = root_indexes.to_vec();
            if schedule & 2 != 0 {
                pending.push(22);
            }
            while let Some(node) = pending.pop() {
                if expected.insert(node) {
                    pending.extend(graph[node].iter().copied());
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
                "seed {seed}, schedule {schedule}"
            );
            assert!(roots.iter().all(|root| arena.get(root.handle()).is_ok()));
        }
    }
}

#[test]
fn wasm_closure_collects_cycle_clears_weak_and_admits_finalizer_without_host_thread() {
    let specimen = || {
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(4).unwrap());
        let owner = arena.allocate(Node::default()).unwrap();
        let cycle_a = arena.allocate(Node::default()).unwrap();
        let cycle_b = arena.allocate(Node::default()).unwrap();
        arena.get_mut(owner).unwrap().weak.push(Some(cycle_a.id()));
        arena.get_mut(cycle_a).unwrap().strong.push(cycle_b.id());
        arena.get_mut(cycle_b).unwrap().strong.push(cycle_a.id());
        let _root = arena.root(owner).unwrap();
        let mut registry = FinalizationRegistry::default();
        registry.register(cycle_a.id());
        let mut jobs = JobQueues::new(AdmissionLimit(1));
        let receipt =
            collect_with_finalization(&mut arena, limits(), &mut registry, &mut jobs, |_| {})
                .unwrap();
        assert_eq!(receipt.swept, vec![cycle_a.id(), cycle_b.id()]);
        assert_eq!(receipt.cleared_weak, vec![(owner.id(), EdgeId(0))]);
        assert_eq!(receipt.finalization.len(), 1);
        assert_eq!(jobs.remaining_admission(), 0);
    };
    specimen();
}

#[test]
fn hard_capped_retain_refuses_at_cap_while_tracing_reclaims_cycles() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(2).unwrap());
    let a = arena.allocate(Node::default()).unwrap();
    let b = arena.allocate(Node::default()).unwrap();
    arena.get_mut(a).unwrap().strong.push(b.id());
    arena.get_mut(b).unwrap().strong.push(a.id());
    assert_eq!(
        arena.allocate(Node::default()).unwrap_err(),
        sim_lib_mutation::ArenaError::CapacityExceeded { cap: 2 }
    );
    assert_eq!(
        collect(&mut arena, limits()).unwrap().swept,
        vec![a.id(), b.id()]
    );
    assert!(arena.allocate(Node::default()).is_ok());
}
