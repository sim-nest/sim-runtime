// conformance: generic incremental query core behavior

use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

use crate::{
    BudgetKind, FingerprintValue, GraphSnapshot, IncrementalEngine, IncrementalError, Observation,
    ObservationKind, QueryBudgets, Revision, SnapshotBudgets, SnapshotError, ValueFingerprint,
};

#[test]
fn nested_reads_record_dependencies_and_reuse_memos() {
    let leaf_runs = Arc::new(AtomicUsize::new(0));
    let branch_runs = Arc::new(AtomicUsize::new(0));
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    let leaf_counter = Arc::clone(&leaf_runs);
    engine.register_fn("leaf", move |_, _| {
        leaf_counter.fetch_add(1, Ordering::Relaxed);
        Ok(2)
    });
    let branch_counter = Arc::clone(&branch_runs);
    engine.register_fn("branch", move |_, frame| {
        branch_counter.fetch_add(1, Ordering::Relaxed);
        Ok(frame.read("leaf")? + 3)
    });

    assert_eq!(engine.verify("branch").unwrap(), 5);
    assert_eq!(engine.verify("branch").unwrap(), 5);
    assert_eq!(leaf_runs.load(Ordering::Relaxed), 1);
    assert_eq!(branch_runs.load(Ordering::Relaxed), 1);
}

#[test]
fn reverse_invalidation_uses_value_cutoff_before_rerunning_dependents() {
    let source_value = Arc::new(AtomicI64::new(1));
    let leaf_runs = Arc::new(AtomicUsize::new(0));
    let root_runs = Arc::new(AtomicUsize::new(0));
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    let source = Arc::clone(&source_value);
    let leaf_counter = Arc::clone(&leaf_runs);
    engine.register_fn("leaf", move |_, frame| {
        leaf_counter.fetch_add(1, Ordering::Relaxed);
        frame.observe_epoch("source")?;
        Ok(source.load(Ordering::Relaxed) % 10)
    });
    let root_counter = Arc::clone(&root_runs);
    engine.register_fn("root", move |_, frame| {
        root_counter.fetch_add(1, Ordering::Relaxed);
        Ok(frame.read("leaf")? * 2)
    });

    assert_eq!(engine.verify("root").unwrap(), 2);
    source_value.store(11, Ordering::Relaxed);
    engine.invalidate(&"source");
    assert_eq!(engine.dirty_keys(), vec!["leaf", "root"]);
    assert_eq!(engine.verify("root").unwrap(), 2);
    assert_eq!(leaf_runs.load(Ordering::Relaxed), 2);
    assert_eq!(root_runs.load(Ordering::Relaxed), 1);

    source_value.store(12, Ordering::Relaxed);
    engine.invalidate(&"source");
    assert_eq!(engine.verify("root").unwrap(), 4);
    assert_eq!(root_runs.load(Ordering::Relaxed), 2);
}

#[test]
fn equal_priority_roots_verify_in_stable_key_order() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("b", |_, _| Ok(2));
    engine.register_fn("a", |_, _| Ok(1));

    let verified = engine.verify_many(["b", "a", "b"]).unwrap();

    assert_eq!(verified, vec![("a", 1), ("b", 2)]);
}

#[test]
fn cycle_errors_include_the_repeated_path() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("a", |_, frame| frame.read("b"));
    engine.register_fn("b", |_, frame| frame.read("a"));

    let err = engine.verify("a").unwrap_err();

    assert_eq!(
        err,
        IncrementalError::Cycle {
            path: vec!["a", "b", "a"]
        }
    );
}

#[test]
fn removed_queries_do_not_reuse_stale_memos() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("a", |_, _| Ok(1));
    assert_eq!(engine.verify("a").unwrap(), 1);

    assert!(engine.remove_query(&"a"));

    assert_eq!(
        engine.verify("a").unwrap_err(),
        IncrementalError::UnknownQuery { key: "a" }
    );
    engine.register_fn("a", |_, _| Ok(2));
    assert_eq!(engine.verify("a").unwrap(), 2);
}

#[test]
fn failed_nested_reads_do_not_poison_the_run_stack() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("leaf", |_, _| Ok(1));
    engine.register_fn("root", |_, frame| {
        assert_eq!(
            frame.read("missing").unwrap_err(),
            IncrementalError::UnknownQuery { key: "missing" }
        );
        assert_eq!(
            frame.read("missing").unwrap_err(),
            IncrementalError::UnknownQuery { key: "missing" }
        );
        Ok(frame.read("leaf")? + 1)
    });

    assert_eq!(engine.verify("root").unwrap(), 2);
}

#[test]
fn budgets_return_typed_errors_and_continuations() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("a", |_, _| Ok(1));
    engine.register_fn("b", |_, frame| Ok(frame.read("a")? + 1));

    let err = engine
        .verify_with_budgets(
            "b",
            QueryBudgets::new(1, usize::MAX, usize::MAX, usize::MAX),
        )
        .unwrap_err();
    let IncrementalError::BudgetExceeded {
        kind,
        continuation: Some(token),
        ..
    } = err
    else {
        panic!("expected work budget with continuation");
    };
    assert_eq!(kind, BudgetKind::Work);
    assert_eq!(engine.resume(token, QueryBudgets::default()).unwrap(), 2);

    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("a", |_, _| Ok(1));
    engine.register_fn("b", |_, frame| Ok(frame.read("a")? + 1));
    let err = engine
        .verify_with_budgets(
            "b",
            QueryBudgets::new(usize::MAX, usize::MAX, 1, usize::MAX),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        IncrementalError::BudgetExceeded {
            kind: BudgetKind::Depth,
            ..
        }
    ));
}

#[test]
fn observation_output_and_cancellation_errors_are_typed() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("observes", |_, frame| {
        frame.observe_missing("missing")?;
        frame.observe_listing("listing")?;
        Ok(1)
    });
    let err = engine
        .verify_with_budgets(
            "observes",
            QueryBudgets::new(usize::MAX, 1, usize::MAX, usize::MAX),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        IncrementalError::BudgetExceeded {
            kind: BudgetKind::Observations,
            ..
        }
    ));

    engine.register_fn("big", |_, frame| {
        frame.charge_output(2)?;
        Ok(1)
    });
    let err = engine
        .verify_with_budgets(
            "big",
            QueryBudgets::new(usize::MAX, usize::MAX, usize::MAX, 1),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        IncrementalError::BudgetExceeded {
            kind: BudgetKind::Output,
            ..
        }
    ));

    engine.register_fn("cancel", |_, frame| {
        frame.cancel();
        frame.charge_work(1)?;
        Ok(1)
    });
    assert_eq!(
        engine.verify("cancel").unwrap_err(),
        IncrementalError::Cancelled
    );
}

#[test]
fn snapshots_are_bounded_and_restore_recovers_partial_graphs() {
    let mut engine = IncrementalEngine::<&'static str, i64>::new();
    engine.register_fn("a", |_, _| Ok(1));
    engine.register_fn("b", |_, frame| Ok(frame.read("a")? + 1));
    assert_eq!(engine.verify("b").unwrap(), 2);

    let too_small = engine.snapshot(["b"], SnapshotBudgets::new(1, usize::MAX));
    assert!(matches!(
        too_small.unwrap_err(),
        IncrementalError::BudgetExceeded {
            kind: BudgetKind::Output,
            ..
        }
    ));

    let mut snapshot = engine
        .snapshot(["b"], SnapshotBudgets::new(usize::MAX, usize::MAX))
        .unwrap();
    assert_eq!(snapshot.nodes.len(), 2);
    snapshot.nodes.retain(|node| node.key != "a");

    let mut restored = IncrementalEngine::<&'static str, i64>::new();
    restored.register_fn("a", |_, _| Ok(1));
    restored.register_fn("b", |_, frame| Ok(frame.read("a")? + 1));
    let report = restored.restore_snapshot(snapshot).unwrap();
    assert_eq!(report.nodes, 1);
    assert_eq!(report.recovered_dirty, 1);
    assert_eq!(restored.verify("b").unwrap(), 2);
}

#[test]
fn restore_counts_each_recovered_dirty_node_once() {
    let snapshot = GraphSnapshot::new(vec![crate::SnapshotNode {
        key: "root",
        revision: Revision::new(1),
        dirty: false,
        value: Some(1_i64),
        fingerprint: Some(ValueFingerprint::new(999)),
        dependencies: vec![
            Observation::read("missing-a", Revision::new(1), ValueFingerprint::new(1)),
            Observation::read("missing-b", Revision::new(1), ValueFingerprint::new(2)),
        ],
    }]);
    let mut engine = IncrementalEngine::<&'static str, i64>::new();

    let report = engine.restore_snapshot(snapshot).unwrap();

    assert_eq!(report.nodes, 1);
    assert_eq!(report.recovered_dirty, 1);
}

#[test]
fn duplicate_snapshot_nodes_are_corruption() {
    let snapshot = GraphSnapshot::new(vec![
        crate::SnapshotNode {
            key: "a",
            revision: crate::Revision::new(1),
            dirty: false,
            value: Some(1_i64),
            fingerprint: Some(1_i64.incremental_fingerprint()),
            dependencies: Vec::new(),
        },
        crate::SnapshotNode {
            key: "a",
            revision: crate::Revision::new(1),
            dirty: false,
            value: Some(1_i64),
            fingerprint: Some(1_i64.incremental_fingerprint()),
            dependencies: Vec::new(),
        },
    ]);
    let mut engine = IncrementalEngine::<&'static str, i64>::new();

    assert_eq!(
        engine.restore_snapshot(snapshot).unwrap_err(),
        SnapshotError::DuplicateNode { key: "a" }
    );
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum DiffKey {
    Query(usize),
    Source(usize),
}

#[test]
fn randomized_differential_matches_full_recomputation() {
    let deps = Arc::new(vec![
        vec![],
        vec![0],
        vec![0, 1],
        vec![1],
        vec![2, 3],
        vec![4],
    ]);
    let sources = Arc::new(
        (0..6)
            .map(|value| AtomicI64::new(value as i64))
            .collect::<Vec<_>>(),
    );
    let mut engine = IncrementalEngine::<DiffKey, i64>::new();
    for index in 0..6 {
        let deps = Arc::clone(&deps);
        let sources = Arc::clone(&sources);
        engine.register_fn(DiffKey::Query(index), move |_, frame| {
            frame.observe(ObservationKind::Epoch, DiffKey::Source(index))?;
            let mut sum = sources[index].load(Ordering::Relaxed);
            for dep in &deps[index] {
                sum += frame.read(DiffKey::Query(*dep))?;
            }
            Ok(sum)
        });
    }

    let mut rng = Lcg::new(0x51_4d_31);
    for _ in 0..96 {
        let index = rng.next_usize(6);
        sources[index].store(rng.next_usize(20) as i64 - 10, Ordering::Relaxed);
        engine.invalidate(&DiffKey::Source(index));
        let expected = full_recompute(5, &deps, &sources);
        assert_eq!(engine.verify(DiffKey::Query(5)).unwrap(), expected);
    }
}

fn full_recompute(index: usize, deps: &[Vec<usize>], sources: &[AtomicI64]) -> i64 {
    sources[index].load(Ordering::Relaxed)
        + deps[index]
            .iter()
            .map(|dep| full_recompute(*dep, deps, sources))
            .sum::<i64>()
}

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_usize(&mut self, limit: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 32) as usize) % limit
    }
}
