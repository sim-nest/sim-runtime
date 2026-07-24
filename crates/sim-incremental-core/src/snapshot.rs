//! Bounded graph snapshot records.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BudgetKind, FingerprintValue, IncrementalEngine, IncrementalError, Observation,
    ObservationKind, QueryResult, Revision, SnapshotBudgets, SnapshotError, ValueFingerprint,
    state::Node,
};

/// A deterministic snapshot of memoized graph state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphSnapshot<K, V> {
    /// Snapshot nodes in stable key order.
    pub nodes: Vec<SnapshotNode<K, V>>,
}

impl<K, V> GraphSnapshot<K, V> {
    /// Creates a graph snapshot from already-ordered nodes.
    #[must_use]
    pub fn new(nodes: Vec<SnapshotNode<K, V>>) -> Self {
        Self { nodes }
    }
}

/// One memo node inside a graph snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotNode<K, V> {
    /// The query key.
    pub key: K,
    /// The memo revision.
    pub revision: Revision,
    /// Whether the memo needs verification before reuse.
    pub dirty: bool,
    /// The memoized value, when one exists.
    pub value: Option<V>,
    /// The memoized value fingerprint.
    pub fingerprint: Option<ValueFingerprint>,
    /// Dependency observations captured during the last execution.
    pub dependencies: Vec<Observation<K>>,
}

/// Summary of a snapshot restore operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestoreReport {
    /// Number of nodes restored.
    pub nodes: usize,
    /// Number of nodes marked dirty to recover from partial or stale snapshot
    /// contents.
    pub recovered_dirty: usize,
}

impl<K, V> IncrementalEngine<K, V>
where
    K: Ord + Clone,
    V: Clone + FingerprintValue,
{
    /// Exports memo graph state reachable from `roots`.
    pub fn snapshot<I>(
        &mut self,
        roots: I,
        budgets: SnapshotBudgets,
    ) -> QueryResult<K, GraphSnapshot<K, V>>
    where
        I: IntoIterator<Item = K>,
    {
        let mut pending = roots.into_iter().collect::<BTreeSet<_>>();
        let Some(root) = pending.iter().next().cloned() else {
            return Ok(GraphSnapshot::new(Vec::new()));
        };
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        let mut edges = 0_usize;
        while let Some(key) = pending.iter().next().cloned() {
            pending.remove(&key);
            if !seen.insert(key.clone()) {
                continue;
            }
            if nodes.len().saturating_add(1) > budgets.max_nodes {
                let token = self.alloc_continuation(root);
                return Err(IncrementalError::BudgetExceeded {
                    kind: BudgetKind::Output,
                    limit: budgets.max_nodes,
                    consumed: nodes.len().saturating_add(1),
                    continuation: Some(token),
                });
            }
            let node = self
                .nodes
                .get(&key)
                .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?;
            edges = edges.saturating_add(node.dependencies.len());
            if edges > budgets.max_edges {
                let token = self.alloc_continuation(root);
                return Err(IncrementalError::BudgetExceeded {
                    kind: BudgetKind::Output,
                    limit: budgets.max_edges,
                    consumed: edges,
                    continuation: Some(token),
                });
            }
            for observation in &node.dependencies {
                if matches!(observation.kind(), ObservationKind::Read) {
                    pending.insert(observation.key().clone());
                }
            }
            nodes.push(SnapshotNode {
                key,
                revision: node.revision,
                dirty: node.dirty,
                value: node.value.clone(),
                fingerprint: node.fingerprint,
                dependencies: node.dependencies.clone(),
            });
        }
        Ok(GraphSnapshot::new(nodes))
    }

    /// Restores memo graph state and rebuilds reverse dependency edges.
    pub fn restore_snapshot(
        &mut self,
        snapshot: GraphSnapshot<K, V>,
    ) -> Result<RestoreReport, SnapshotError<K>> {
        let mut keys = BTreeSet::new();
        for node in &snapshot.nodes {
            if !keys.insert(node.key.clone()) {
                return Err(SnapshotError::DuplicateNode {
                    key: node.key.clone(),
                });
            }
        }

        self.nodes.clear();
        self.reverse.clear();
        let mut recovered_dirty = 0_usize;
        let mut max_revision = self.next_revision.saturating_sub(1);
        for snapshot_node in snapshot.nodes {
            max_revision = max_revision.max(snapshot_node.revision.get());
            let mut dirty = snapshot_node.dirty;
            let mut recovered = false;
            if snapshot_node.value.is_none() {
                recovered = !dirty;
                dirty = true;
            }
            let mut fingerprint = snapshot_node.fingerprint;
            if let Some(value) = &snapshot_node.value {
                let computed = value.incremental_fingerprint();
                if fingerprint != Some(computed) {
                    fingerprint = Some(computed);
                    recovered = recovered || !dirty;
                    dirty = true;
                }
            }
            if has_missing_read_deps(&snapshot_node, &keys) {
                recovered = recovered || !dirty;
                dirty = true;
            }
            if recovered {
                recovered_dirty += 1;
            }
            restore_external_revisions(&mut self.source_revisions, &snapshot_node);
            self.nodes.insert(
                snapshot_node.key,
                Node {
                    revision: snapshot_node.revision,
                    dirty,
                    value: snapshot_node.value,
                    fingerprint,
                    dependencies: snapshot_node.dependencies,
                },
            );
        }
        self.next_revision = self.next_revision.max(max_revision.saturating_add(1));
        self.rebuild_reverse();
        Ok(RestoreReport {
            nodes: self.nodes.len(),
            recovered_dirty,
        })
    }

    fn rebuild_reverse(&mut self) {
        self.reverse.clear();
        for (key, node) in &self.nodes {
            for observation in &node.dependencies {
                self.reverse
                    .entry(observation.key().clone())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
}

fn has_missing_read_deps<K, V>(snapshot_node: &SnapshotNode<K, V>, keys: &BTreeSet<K>) -> bool
where
    K: Ord,
{
    snapshot_node.dependencies.iter().any(|observation| {
        matches!(observation.kind(), ObservationKind::Read) && !keys.contains(observation.key())
    })
}

fn restore_external_revisions<K, V>(
    source_revisions: &mut BTreeMap<K, Revision>,
    snapshot_node: &SnapshotNode<K, V>,
) where
    K: Ord + Clone,
{
    for observation in &snapshot_node.dependencies {
        if matches!(observation.kind(), ObservationKind::Read) {
            continue;
        }
        source_revisions
            .entry(observation.key().clone())
            .and_modify(|revision| {
                if observation.revision() > *revision {
                    *revision = observation.revision();
                }
            })
            .or_insert(observation.revision());
    }
}
