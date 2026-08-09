#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bounded stop-the-world tracing collection for managed arenas.

use std::{collections::BTreeSet, error::Error, fmt};

use sim_lib_mutation::{ArenaError, EdgeId, EdgeVisitor, ManagedArena, ManagedId, ManagedObject};

/// Independently enforced limits for one collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionLimits {
    /// Maximum arena objects admitted.
    pub objects: usize,
    /// Maximum enumerated edges admitted.
    pub edges: usize,
    /// Maximum pending iterative mark stack length.
    pub stack: usize,
    /// Maximum charged root, object, edge, ephemeron, and sweep operations.
    pub work: usize,
}

/// A resource class which refused collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    /// Arena objects.
    Objects,
    /// Enumerated edges.
    Edges,
    /// Pending mark entries.
    Stack,
    /// Total charged operations.
    Work,
}

/// Inspectable evidence for a collection refused before mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureReceipt {
    /// Snapshot mutation epoch.
    pub mutation_epoch: u64,
    /// Exhausted resource.
    pub kind: LimitKind,
    /// Configured maximum.
    pub limit: usize,
    /// Required amount at refusal.
    pub required: usize,
}

/// Deterministic evidence for a completed collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionReceipt {
    /// Snapshot mutation epoch used to plan the sweep.
    pub mutation_epoch: u64,
    /// Reachable objects in allocation order.
    pub marked: Vec<ManagedId>,
    /// Reclaimed objects in allocation order.
    pub swept: Vec<ManagedId>,
    /// Number of edges enumerated.
    pub edges: usize,
    /// Total charged operations.
    pub work: usize,
}

/// A fail-closed collection error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollectionError {
    /// A budget could not admit the complete read-only plan.
    Limit(FailureReceipt),
    /// The arena rejected a stale edge or atomic sweep.
    Arena(ArenaError),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(r) => write!(
                f,
                "collection {:?} limit {} requires {}",
                r.kind, r.limit, r.required
            ),
            Self::Arena(error) => error.fmt(f),
        }
    }
}
impl Error for CollectionError {}
impl From<ArenaError> for CollectionError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}

#[derive(Default)]
struct Edges {
    strong: Vec<ManagedId>,
    ephemerons: Vec<(ManagedId, ManagedId)>,
    count: usize,
}
impl EdgeVisitor for Edges {
    fn strong(&mut self, _: EdgeId, target: ManagedId) {
        self.count += 1;
        self.strong.push(target);
    }
    fn weak(&mut self, _: EdgeId, _: ManagedId) {
        self.count += 1;
    }
    fn ephemeron(&mut self, _: EdgeId, key: ManagedId, value: ManagedId) {
        self.count += 1;
        self.ephemerons.push((key, value));
    }
}

fn charge(
    epoch: u64,
    kind: LimitKind,
    limit: usize,
    required: usize,
) -> Result<(), CollectionError> {
    if required > limit {
        Err(CollectionError::Limit(FailureReceipt {
            mutation_epoch: epoch,
            kind,
            limit,
            required,
        }))
    } else {
        Ok(())
    }
}

/// Performs a complete bounded collection, mutating only after the plan succeeds.
pub fn collect<T: ManagedObject>(
    arena: &mut ManagedArena<T>,
    limits: CollectionLimits,
) -> Result<CollectionReceipt, CollectionError> {
    let (plan, _) = arena.safepoint(|snapshot| {
        let epoch = snapshot.mutation_epoch();
        let all = snapshot.objects().collect::<Vec<_>>();
        charge(epoch, LimitKind::Objects, limits.objects, all.len())?;
        let mut marked = BTreeSet::new();
        let mut pending = snapshot.roots().collect::<Vec<_>>();
        charge(epoch, LimitKind::Stack, limits.stack, pending.len())?;
        let mut edge_count = 0usize;
        let mut work = pending.len();
        charge(epoch, LimitKind::Work, limits.work, work)?;
        let mut ephemerons = Vec::new();
        while let Some(id) = pending.pop() {
            if !marked.insert(id) {
                continue;
            }
            let mut found = Edges::default();
            snapshot.visit_edges(id, &mut found)?;
            edge_count = edge_count.saturating_add(found.count);
            charge(epoch, LimitKind::Edges, limits.edges, edge_count)?;
            work = work.saturating_add(1 + found.count);
            charge(epoch, LimitKind::Work, limits.work, work)?;
            for target in found.strong {
                snapshot.visit_edges(target, &mut Edges::default())?;
                if !marked.contains(&target) {
                    pending.push(target);
                }
            }
            charge(epoch, LimitKind::Stack, limits.stack, pending.len())?;
            ephemerons.extend(found.ephemerons);
        }
        loop {
            let mut changed = false;
            for &(key, value) in &ephemerons {
                work = work.saturating_add(1);
                charge(epoch, LimitKind::Work, limits.work, work)?;
                if marked.contains(&key) && !marked.contains(&value) {
                    pending.push(value);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            while let Some(id) = pending.pop() {
                if !marked.insert(id) {
                    continue;
                }
                let mut found = Edges::default();
                snapshot.visit_edges(id, &mut found)?;
                edge_count = edge_count.saturating_add(found.count);
                charge(epoch, LimitKind::Edges, limits.edges, edge_count)?;
                work = work.saturating_add(1 + found.count);
                charge(epoch, LimitKind::Work, limits.work, work)?;
                for target in found.strong {
                    snapshot.visit_edges(target, &mut Edges::default())?;
                    if !marked.contains(&target) {
                        pending.push(target);
                    }
                }
                ephemerons.extend(found.ephemerons);
                charge(epoch, LimitKind::Stack, limits.stack, pending.len())?;
            }
        }
        let swept = all
            .iter()
            .copied()
            .filter(|id| !marked.contains(id))
            .collect::<Vec<_>>();
        work = work.saturating_add(swept.len());
        charge(epoch, LimitKind::Work, limits.work, work)?;
        Ok::<_, CollectionError>((epoch, all, marked, edge_count, work))
    })?;
    let (epoch, all, marked, edges, work) = plan?;
    let swept = all
        .into_iter()
        .filter(|id| !marked.contains(id))
        .collect::<Vec<_>>();
    arena.sweep_at_epoch(epoch, &swept)?;
    Ok(CollectionReceipt {
        mutation_epoch: epoch,
        marked: marked.into_iter().collect(),
        swept,
        edges,
        work,
    })
}

#[cfg(test)]
mod tests;
