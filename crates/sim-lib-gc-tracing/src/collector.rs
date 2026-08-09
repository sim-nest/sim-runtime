use std::collections::BTreeSet;

use sim_lib_control::{CheckpointError, JobQueues, RuntimeJobClass};
use sim_lib_mutation::{EdgeId, EdgeVisitor, ManagedArena, ManagedId, ManagedObject};

use crate::{
    CollectionError, CollectionLimits, CollectionReceipt, FailureReceipt, FinalizationRecord,
    FinalizationRegistry, LimitKind,
};

#[derive(Default)]
struct Edges {
    strong: Vec<ManagedId>,
    weak: Vec<(ManagedId, EdgeId, ManagedId)>,
    ephemerons: Vec<(ManagedId, EdgeId, ManagedId, ManagedId)>,
    owner: Option<ManagedId>,
    count: usize,
}
impl EdgeVisitor for Edges {
    fn strong(&mut self, _: EdgeId, target: ManagedId) {
        self.count += 1;
        self.strong.push(target);
    }
    fn weak(&mut self, edge: EdgeId, target: ManagedId) {
        self.count += 1;
        self.weak
            .push((self.owner.expect("visitor owner"), edge, target));
    }
    fn ephemeron(&mut self, edge: EdgeId, key: ManagedId, value: ManagedId) {
        self.count += 1;
        self.ephemerons
            .push((self.owner.expect("visitor owner"), edge, key, value));
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
    collect_with_finalization(
        arena,
        limits,
        &mut FinalizationRegistry::default(),
        &mut JobQueues::new(sim_lib_control::AdmissionLimit(0)),
        |_| {},
    )
}

/// Collects and atomically admits ready records to the isolated finalization queue.
/// The callback is stored as a job and cannot run before this function returns.
pub fn collect_with_finalization<T: ManagedObject, F: Fn(FinalizationRecord) + Clone + 'static>(
    arena: &mut ManagedArena<T>,
    limits: CollectionLimits,
    registry: &mut FinalizationRegistry,
    jobs: &mut JobQueues<RuntimeJobClass>,
    run: F,
) -> Result<CollectionReceipt, CollectionError> {
    let (plan, _) = arena.safepoint(|snapshot| {
        let epoch = snapshot.mutation_epoch();
        let all = snapshot.objects().collect::<Vec<_>>();
        charge(epoch, LimitKind::Objects, limits.objects, all.len())?;
        let mut marked = BTreeSet::new();
        let mut pending = snapshot.roots().collect::<Vec<_>>();
        pending.extend(snapshot.kept_alive());
        charge(epoch, LimitKind::Stack, limits.stack, pending.len())?;
        let mut edge_count = 0usize;
        let mut work = pending.len();
        charge(epoch, LimitKind::Work, limits.work, work)?;
        let mut ephemerons = Vec::new();
        let mut weak = Vec::new();
        while let Some(id) = pending.pop() {
            if !marked.insert(id) {
                continue;
            }
            let mut found = Edges {
                owner: Some(id),
                ..Edges::default()
            };
            snapshot.visit_edges(id, &mut found)?;
            edge_count = edge_count.saturating_add(found.count);
            charge(epoch, LimitKind::Edges, limits.edges, edge_count)?;
            work = work.saturating_add(1 + found.count);
            charge(epoch, LimitKind::Work, limits.work, work)?;
            for target in found.strong {
                if !marked.contains(&target) {
                    pending.push(target);
                }
            }
            charge(epoch, LimitKind::Stack, limits.stack, pending.len())?;
            ephemerons.extend(found.ephemerons);
            weak.extend(found.weak);
        }
        loop {
            let mut changed = false;
            for &(_, _, key, value) in &ephemerons {
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
                let mut found = Edges {
                    owner: Some(id),
                    ..Edges::default()
                };
                snapshot.visit_edges(id, &mut found)?;
                edge_count = edge_count.saturating_add(found.count);
                charge(epoch, LimitKind::Edges, limits.edges, edge_count)?;
                work = work.saturating_add(1 + found.count);
                charge(epoch, LimitKind::Work, limits.work, work)?;
                for target in found.strong {
                    if !marked.contains(&target) {
                        pending.push(target);
                    }
                }
                ephemerons.extend(found.ephemerons);
                weak.extend(found.weak);
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
        let weak_clear = weak
            .into_iter()
            .filter(|(_, _, target)| !marked.contains(target))
            .collect::<Vec<_>>();
        let eph_clear = ephemerons
            .into_iter()
            .filter(|(_, _, key, _)| !marked.contains(key))
            .collect::<Vec<_>>();
        charge(
            epoch,
            LimitKind::Clears,
            limits.clears,
            weak_clear.len().saturating_add(eph_clear.len()),
        )?;
        Ok::<_, CollectionError>((epoch, all, marked, edge_count, work, weak_clear, eph_clear))
    })?;
    let (epoch, all, marked, edges, work, weak, ephemerons) = plan?;
    let swept = all
        .into_iter()
        .filter(|id| !marked.contains(id))
        .collect::<Vec<_>>();
    let records = registry.ready(&swept.iter().copied().collect());
    charge(
        epoch,
        LimitKind::Finalizers,
        limits.finalizers,
        records.len(),
    )?;
    if records.len() > jobs.remaining_admission() {
        return Err(CollectionError::Limit(FailureReceipt {
            mutation_epoch: epoch,
            kind: LimitKind::Finalizers,
            limit: jobs.remaining_admission(),
            required: records.len(),
        }));
    }
    let mutation = arena.apply_collection_at_epoch(epoch, &weak, &ephemerons, &swept)?;
    for record in &records {
        let callback = run.clone();
        let value = *record;
        jobs.enqueue(RuntimeJobClass::Finalization, move |_| callback(value))
            .map_err(|error| match error {
                CheckpointError::AdmissionExhausted | CheckpointError::WorkExhausted => {
                    CollectionError::Limit(FailureReceipt {
                        mutation_epoch: epoch,
                        kind: LimitKind::Finalizers,
                        limit: 0,
                        required: 1,
                    })
                }
            })?;
    }
    registry.mark_admitted(&records);
    Ok(CollectionReceipt {
        mutation_epoch: epoch,
        marked: marked.into_iter().collect(),
        swept: mutation.swept,
        edges,
        work,
        cleared_weak: mutation.cleared_weak,
        cleared_ephemerons: mutation.cleared_ephemerons,
        finalization: records,
    })
}
