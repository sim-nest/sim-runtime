//! Managed, revision-checked caches for derived class views.

use std::{collections::BTreeMap, error::Error, fmt};

use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, collect};
use sim_lib_mutation::{
    ArenaError, EdgeId, EdgeVisitor, EphemeronMutationError, HardCappedRetainPolicy, ManagedArena,
    ManagedHandle, ManagedId, ManagedNode, ManagedObject, RootedHandle, StrongEdgeMutationError,
};

use crate::{LineageBudget, LineageError, LineageGraph, LineagePolicy};

/// Parent and member revisions observed for one class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheRevisions {
    /// Revision of the declared-parent list.
    pub parents: u64,
    /// Revision of the declared-member list.
    pub members: u64,
}

/// A cached class linearization and its root-first derived member view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedClassView<M> {
    /// The class followed by its ancestors in policy order.
    pub linearization: Vec<ManagedId>,
    /// Members concatenated in the same order as `linearization`.
    pub members: Vec<M>,
}

/// Whether an access reused or recomputed a derived value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAccessKind {
    /// All lineage revision stamps still matched.
    Hit,
    /// No value existed, or at least one lineage revision changed.
    Recomputed,
}

/// Inspectable evidence for one cache access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheAccess<M> {
    /// Access disposition.
    pub kind: CacheAccessKind,
    /// The observed derived value.
    pub view: DerivedClassView<M>,
}

/// A strong root for a managed class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassRoot(RootedHandle);

impl ClassRoot {
    /// Returns the managed class identity.
    pub const fn id(self) -> ManagedId {
        self.0.handle().id()
    }
}

#[derive(Clone, Debug)]
enum Object<M> {
    Manager(ManagedNode<()>),
    Class {
        edges: ManagedNode<()>,
        parents: Vec<ManagedHandle>,
        members: Vec<M>,
        revisions: CacheRevisions,
        cached: Option<CachedRef>,
    },
    Derived {
        edges: ManagedNode<()>,
        stamps: Vec<(ManagedId, CacheRevisions)>,
        view: DerivedClassView<M>,
    },
}

#[derive(Clone, Copy, Debug)]
struct CachedRef {
    edge: EdgeId,
    value: ManagedHandle,
}

impl<M> Object<M> {
    fn edges(&self) -> &ManagedNode<()> {
        match self {
            Self::Manager(edges) | Self::Class { edges, .. } | Self::Derived { edges, .. } => edges,
        }
    }
    fn edges_mut(&mut self) -> &mut ManagedNode<()> {
        match self {
            Self::Manager(edges) | Self::Class { edges, .. } | Self::Derived { edges, .. } => edges,
        }
    }
}

impl<M> ManagedObject for Object<M> {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        self.edges().trace_edges(visitor);
    }
    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        self.edges_mut().clear_weak_edge(edge, expected)
    }
    fn clear_ephemeron_edge(&mut self, edge: EdgeId, key: ManagedId, value: ManagedId) -> bool {
        self.edges_mut().clear_ephemeron_edge(edge, key, value)
    }
}

/// Failure from checked class-cache mutation or computation.
#[derive(Debug)]
pub enum CacheError {
    /// A managed-arena operation failed.
    Arena(ArenaError),
    /// A parent edge mutation failed.
    Strong(StrongEdgeMutationError),
    /// An ephemeron mutation failed.
    Ephemeron(EphemeronMutationError),
    /// The lineage policy rejected the graph.
    Lineage(LineageError<ManagedId>),
    /// Collection failed before completing atomically.
    Collection(CollectionError),
    /// A handle named a managed object of the wrong role.
    WrongObject(ManagedId),
    /// A revision counter exhausted its identity space.
    RevisionExhausted(ManagedId),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for CacheError {}
impl From<ArenaError> for CacheError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}
impl From<StrongEdgeMutationError> for CacheError {
    fn from(value: StrongEdgeMutationError) -> Self {
        Self::Strong(value)
    }
}
impl From<EphemeronMutationError> for CacheError {
    fn from(value: EphemeronMutationError) -> Self {
        Self::Ephemeron(value)
    }
}
impl From<CollectionError> for CacheError {
    fn from(value: CollectionError) -> Self {
        Self::Collection(value)
    }
}

/// A bounded managed class universe with non-retaining derived caches.
pub struct ClassCache<M> {
    arena: ManagedArena<Object<M>>,
    manager: RootedHandle,
}

impl<M: Clone> ClassCache<M> {
    /// Creates a cache with a hard cap shared by classes and derived values.
    pub fn new(max_objects: usize) -> Result<Self, CacheError> {
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(max_objects)?);
        let manager = arena.allocate(Object::Manager(ManagedNode::new(())))?;
        let manager = arena.root(manager)?;
        Ok(Self { arena, manager })
    }

    /// Allocates and roots a class with its declared parents and members.
    pub fn allocate_class(
        &mut self,
        parents: &[ClassRoot],
        members: Vec<M>,
    ) -> Result<ClassRoot, CacheError> {
        let mut edges = ManagedNode::new(());
        let parent_handles = parents
            .iter()
            .map(|parent| parent.0.handle())
            .collect::<Vec<_>>();
        for parent in &parent_handles {
            edges.insert_strong(parent.id())?;
        }
        let handle = self.arena.allocate(Object::Class {
            edges,
            parents: parent_handles,
            members,
            revisions: CacheRevisions {
                parents: 0,
                members: 0,
            },
            cached: None,
        })?;
        Ok(ClassRoot(self.arena.root(handle)?))
    }

    /// Replaces declared parents and bumps the parent revision.
    pub fn replace_parents(
        &mut self,
        class: ClassRoot,
        parents: &[ClassRoot],
    ) -> Result<(), CacheError> {
        self.discard_cached(class.0.handle())?;
        let object = self.arena.get_mut(class.0.handle())?;
        let Object::Class {
            edges,
            parents: stored,
            revisions,
            ..
        } = object
        else {
            return Err(CacheError::WrongObject(class.id()));
        };
        *edges = ManagedNode::new(());
        stored.clear();
        for parent in parents {
            edges.insert_strong(parent.id())?;
            stored.push(parent.0.handle());
        }
        revisions.parents = revisions
            .parents
            .checked_add(1)
            .ok_or(CacheError::RevisionExhausted(class.id()))?;
        Ok(())
    }

    /// Replaces declared members and bumps the member revision.
    pub fn replace_members(&mut self, class: ClassRoot, members: Vec<M>) -> Result<(), CacheError> {
        self.discard_cached(class.0.handle())?;
        let object = self.arena.get_mut(class.0.handle())?;
        let Object::Class {
            members: stored,
            revisions,
            ..
        } = object
        else {
            return Err(CacheError::WrongObject(class.id()));
        };
        *stored = members;
        revisions.members = revisions
            .members
            .checked_add(1)
            .ok_or(CacheError::RevisionExhausted(class.id()))?;
        Ok(())
    }

    /// Returns a revision-validated cached view or computes and installs one.
    pub fn derived<P>(
        &mut self,
        class: ClassRoot,
        policy: &P,
        budget: LineageBudget,
    ) -> Result<CacheAccess<M>, CacheError>
    where
        P: LineagePolicy<SnapshotGraph<M>>,
    {
        let graph = self.snapshot_graph()?;
        if let Some(cached) = self.cached(class.0.handle())? {
            let Object::Derived { stamps, view, .. } = self.arena.get(cached.value)? else {
                return Err(CacheError::WrongObject(cached.value.id()));
            };
            if stamps
                .iter()
                .all(|(id, expected)| graph.revisions.get(id) == Some(expected))
            {
                return Ok(CacheAccess {
                    kind: CacheAccessKind::Hit,
                    view: view.clone(),
                });
            }
        }
        self.discard_cached(class.0.handle())?;
        let linearization = policy
            .linearize(&graph, &class.id(), budget)
            .map_err(CacheError::Lineage)?;
        let stamps = linearization
            .iter()
            .map(|id| (*id, graph.revisions[id]))
            .collect::<Vec<_>>();
        let members = linearization
            .iter()
            .flat_map(|id| graph.members[id].clone())
            .collect();
        let view = DerivedClassView {
            linearization,
            members,
        };
        let value = self.arena.allocate(Object::Derived {
            edges: ManagedNode::new(()),
            stamps,
            view: view.clone(),
        })?;
        let edge = match self.arena.get_mut(self.manager.handle())? {
            Object::Manager(edges) => edges.insert_ephemeron(class.id(), value.id())?,
            _ => return Err(CacheError::WrongObject(self.manager.handle().id())),
        };
        let Object::Class { cached, .. } = self.arena.get_mut(class.0.handle())? else {
            return Err(CacheError::WrongObject(class.id()));
        };
        *cached = Some(CachedRef { edge, value });
        Ok(CacheAccess {
            kind: CacheAccessKind::Recomputed,
            view,
        })
    }

    /// Releases the caller's strong root. Collection can then reclaim the class and cache value.
    pub fn release(&mut self, class: ClassRoot) -> Result<(), CacheError> {
        self.arena.release_root(class.0)?;
        Ok(())
    }

    /// Runs bounded MANAGED_2 tracing collection and returns its exact receipt.
    pub fn collect(&mut self, limits: CollectionLimits) -> Result<CollectionReceipt, CacheError> {
        Ok(collect(&mut self.arena, limits)?)
    }

    /// Returns the current number of managed manager, class, and derived objects.
    pub fn managed_len(&self) -> usize {
        self.arena.len()
    }

    fn cached(&self, class: ManagedHandle) -> Result<Option<CachedRef>, CacheError> {
        match self.arena.get(class)? {
            Object::Class { cached, .. } => Ok(*cached),
            _ => Err(CacheError::WrongObject(class.id())),
        }
    }

    fn discard_cached(&mut self, class: ManagedHandle) -> Result<(), CacheError> {
        let Some(cached) = self.cached(class)? else {
            return Ok(());
        };
        match self.arena.get_mut(self.manager.handle())? {
            Object::Manager(edges) => {
                edges.remove_ephemeron(cached.edge, (class.id(), cached.value.id()))?;
            }
            _ => return Err(CacheError::WrongObject(self.manager.handle().id())),
        }
        self.arena.remove(cached.value)?;
        let Object::Class { cached, .. } = self.arena.get_mut(class)? else {
            return Err(CacheError::WrongObject(class.id()));
        };
        *cached = None;
        Ok(())
    }

    fn snapshot_graph(&mut self) -> Result<SnapshotGraph<M>, CacheError> {
        let mut graph = SnapshotGraph::default();
        let (ids, _) = self
            .arena
            .safepoint(|snapshot| snapshot.objects().collect::<Vec<_>>())?;
        for id in ids {
            let handle = self.arena.handle(id)?;
            if let Object::Class {
                parents,
                members,
                revisions,
                ..
            } = self.arena.get(handle)?
            {
                graph
                    .parents
                    .insert(id, parents.iter().map(|parent| parent.id()).collect());
                graph.members.insert(id, members.clone());
                graph.revisions.insert(id, *revisions);
            }
        }
        Ok(graph)
    }
}

/// Immutable computation snapshot; public only as the policy trait's graph parameter.
pub struct SnapshotGraph<M = ()> {
    parents: BTreeMap<ManagedId, Vec<ManagedId>>,
    members: BTreeMap<ManagedId, Vec<M>>,
    revisions: BTreeMap<ManagedId, CacheRevisions>,
}
impl<M> Default for SnapshotGraph<M> {
    fn default() -> Self {
        Self {
            parents: BTreeMap::new(),
            members: BTreeMap::new(),
            revisions: BTreeMap::new(),
        }
    }
}
impl<M> LineageGraph for SnapshotGraph<M> {
    type Node = ManagedId;
    fn declared_parents(&self, node: &ManagedId) -> Vec<ManagedId> {
        self.parents.get(node).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::C3Policy;

    fn budget() -> LineageBudget {
        LineageBudget {
            nodes: 32,
            work: 512,
        }
    }
    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 32,
            edges: 32,
            stack: 32,
            work: 512,
            clears: 32,
            finalizers: 0,
        }
    }

    #[test]
    fn hit_matches_recomputation_and_lineage_revisions_invalidate_descendants() {
        let mut cache = ClassCache::new(16).unwrap();
        let parent = cache.allocate_class(&[], vec!["parent-v1"]).unwrap();
        let child = cache.allocate_class(&[parent], vec!["child"]).unwrap();

        let first = cache.derived(child, &C3Policy, budget()).unwrap();
        let hit = cache.derived(child, &C3Policy, budget()).unwrap();
        assert_eq!(first.kind, CacheAccessKind::Recomputed);
        assert_eq!(hit.kind, CacheAccessKind::Hit);
        assert_eq!(hit.view, first.view);

        cache.replace_members(parent, vec!["parent-v2"]).unwrap();
        let recomputed = cache.derived(child, &C3Policy, budget()).unwrap();
        assert_eq!(recomputed.kind, CacheAccessKind::Recomputed);
        assert_eq!(recomputed.view.linearization, first.view.linearization);
        assert_eq!(recomputed.view.members, ["child", "parent-v2"]);

        cache.replace_parents(child, &[]).unwrap();
        let without_parent = cache.derived(child, &C3Policy, budget()).unwrap();
        assert_eq!(without_parent.kind, CacheAccessKind::Recomputed);
        assert_eq!(without_parent.view.linearization, [child.id()]);
        assert_eq!(without_parent.view.members, ["child"]);
    }

    #[test]
    fn dropping_last_class_roots_clears_ephemeron_and_reclaims_cached_value() {
        let mut cache = ClassCache::new(16).unwrap();
        let parent = cache.allocate_class(&[], vec!["parent"]).unwrap();
        let child = cache.allocate_class(&[parent], vec!["child"]).unwrap();
        cache.derived(child, &C3Policy, budget()).unwrap();
        assert_eq!(cache.managed_len(), 4);

        cache.release(parent).unwrap();
        cache.release(child).unwrap();
        let receipt = cache.collect(limits()).unwrap();
        assert_eq!(receipt.cleared_ephemerons.len(), 1);
        assert_eq!(receipt.swept.len(), 3);
        assert_eq!(cache.managed_len(), 1);
    }
}
