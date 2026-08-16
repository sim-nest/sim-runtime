//! Bounded, revision-guarded JVM outcome caches over managed ephemerons.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use sim_lib_mutation::ManagedHandle;

use crate::{
    ClassDefinition, ClassLoaderId, ClassSpaceRevision, JvmEdge, JvmGraphError, JvmHeap, JvmRole,
};

/// The independently observable JVM outcomes admitted to guarded caching.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum GuardedCacheKind {
    /// A literal or other constant-pool outcome.
    Constant,
    /// A resolved class outcome.
    Class,
    /// A resolved field outcome.
    Field,
    /// A resolved class method outcome.
    Method,
    /// A resolved interface method outcome.
    InterfaceMethod,
    /// A linked dynamic call-site outcome.
    DynamicSite,
    /// A decoded, verified, and prepared method body.
    PreparedCode,
}

/// Exact mutable identities which authorize reuse of one cached outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheGuard {
    loader: ClassSpaceRevision,
    class: u64,
    method: u64,
}

impl CacheGuard {
    /// Binds a guard to one loader state, class content revision, and method revision.
    pub const fn new(loader: ClassSpaceRevision, class: u64, method: u64) -> Self {
        Self {
            loader,
            class,
            method,
        }
    }

    /// Loader namespace named by this guard.
    pub const fn loader(self) -> ClassLoaderId {
        self.loader.loader()
    }
}

#[derive(Clone)]
struct Entry<V, E> {
    owner: Weak<ClassDefinition>,
    guard: CacheGuard,
    outcome: Result<V, E>,
    _managed_value: ManagedHandle,
}

type EntryMap<K, V, E> = BTreeMap<(GuardedCacheKind, K), Vec<Entry<V, E>>>;

/// One bounded polymorphic cache shared by resolution, linkage, and preparation.
///
/// A site may observe several defining loaders, but never more than `ways`. Entries
/// are managed ephemerons keyed by the owning class mirror and are also weak on the
/// host side, so neither representation keeps a loader namespace alive.
pub struct GuardedOutcomeCache<K, V, E> {
    ways: usize,
    entries: Mutex<EntryMap<K, V, E>>,
}

impl<K: Clone + Ord, V: Clone, E: Clone> GuardedOutcomeCache<K, V, E> {
    /// Creates a cache with an exact, non-zero polymorphic bound per site.
    pub fn new(ways: usize) -> Self {
        assert!(ways != 0, "a JVM cache must admit at least one way");
        Self {
            ways,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Returns an outcome only when all three revision guards still match.
    pub fn get(&self, kind: GuardedCacheKind, site: &K, guard: CacheGuard) -> Option<Result<V, E>> {
        let mut entries = self.entries();
        Self::purge(&mut entries);
        entries.get(&(kind, site.clone())).and_then(|ways| {
            ways.iter()
                .find(|entry| entry.guard == guard)
                .map(|entry| entry.outcome.clone())
        })
    }

    /// Stores a successful or normative failed outcome as a managed ephemeron.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &self,
        heap: &mut JvmHeap,
        cache: ManagedHandle,
        owner_handle: ManagedHandle,
        kind: GuardedCacheKind,
        site: K,
        owner: &Arc<ClassDefinition>,
        guard: CacheGuard,
        outcome: Result<V, E>,
    ) -> Result<(), JvmGraphError> {
        let managed_value = heap.allocate(JvmRole::Cache).map_err(JvmGraphError::from)?;
        heap.ephemeron(cache, JvmEdge::DerivedEntry, owner_handle, managed_value)?;
        let mut entries = self.entries();
        Self::purge(&mut entries);
        let ways = entries.entry((kind, site)).or_default();
        if let Some(position) = ways.iter().position(|entry| entry.guard == guard) {
            ways.remove(position);
        } else if ways.len() == self.ways {
            ways.remove(0);
        }
        ways.push(Entry {
            owner: Arc::downgrade(owner),
            guard,
            outcome,
            _managed_value: managed_value,
        });
        Ok(())
    }

    /// Number of live host-side entries after weak-owner reclamation.
    pub fn live_len(&self) -> usize {
        let mut entries = self.entries();
        Self::purge(&mut entries);
        entries.values().map(Vec::len).sum()
    }

    /// Number of live variants retained for one cache kind and site.
    pub fn variants(&self, kind: GuardedCacheKind, site: &K) -> usize {
        let mut entries = self.entries();
        Self::purge(&mut entries);
        entries.get(&(kind, site.clone())).map_or(0, Vec::len)
    }

    fn purge(entries: &mut EntryMap<K, V, E>) {
        entries.retain(|_, ways| {
            ways.retain(|entry| entry.owner.strong_count() != 0);
            !ways.is_empty()
        });
    }

    fn entries(&self) -> MutexGuard<'_, EntryMap<K, V, E>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassDefinition, ClassLoader, JavaClassMetadata};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_gc_tracing::CollectionLimits;
    use std::{collections::BTreeMap, sync::Arc};

    fn definition(loader: &ClassLoader, name: &str, key: u64) -> Arc<ClassDefinition> {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        ClassDefinition::test(
            loader.id(),
            name,
            key,
            JavaClassMetadata::test_identity(&cx, name, &[]),
            BTreeMap::new(),
        )
    }

    #[test]
    fn every_cache_kind_is_revision_guarded_bounded_and_mortal() {
        let kinds = [
            GuardedCacheKind::Constant,
            GuardedCacheKind::Class,
            GuardedCacheKind::Field,
            GuardedCacheKind::Method,
            GuardedCacheKind::InterfaceMethod,
            GuardedCacheKind::DynamicSite,
            GuardedCacheKind::PreparedCode,
        ];
        for kind in kinds {
            let loader = ClassLoader::new(1);
            let owner = definition(&loader, "Owner", 11);
            loader.test_insert(owner.clone());
            let mut heap = JvmHeap::new(
                32,
                CollectionLimits {
                    objects: 32,
                    edges: 32,
                    stack: 32,
                    work: 128,
                    clears: 32,
                    finalizers: 0,
                },
            )
            .unwrap();
            let cache_handle = heap.allocate(JvmRole::Cache).unwrap();
            let owner_handle = heap.allocate(JvmRole::ClassMirror).unwrap();
            let cache_root = heap.root(cache_handle).unwrap();
            let owner_root = heap.root(owner_handle).unwrap();
            let cache = GuardedOutcomeCache::<u16, u16, &'static str>::new(2);
            let guard = CacheGuard::new(loader.revision(), 11, 1);
            cache
                .insert(
                    &mut heap,
                    cache_handle,
                    owner_handle,
                    kind,
                    7,
                    &owner,
                    guard,
                    Ok(1),
                )
                .unwrap();
            cache
                .insert(
                    &mut heap,
                    cache_handle,
                    owner_handle,
                    kind,
                    7,
                    &owner,
                    CacheGuard::new(loader.revision(), 11, 2),
                    Err("normative"),
                )
                .unwrap();
            cache
                .insert(
                    &mut heap,
                    cache_handle,
                    owner_handle,
                    kind,
                    7,
                    &owner,
                    CacheGuard::new(loader.revision(), 11, 3),
                    Ok(3),
                )
                .unwrap();
            assert_eq!(
                cache.variants(kind, &7),
                2,
                "{kind:?} exceeded its polymorphic bound"
            );
            assert!(
                cache.get(kind, &7, guard).is_none(),
                "{kind:?} retained an evicted way"
            );
            loader.simulate_class_space_change();
            assert!(
                cache
                    .get(kind, &7, CacheGuard::new(loader.revision(), 11, 3))
                    .is_none(),
                "{kind:?} survived loader mutation"
            );
            assert!(
                cache
                    .get(kind, &7, CacheGuard::new(guard.loader, 12, 3))
                    .is_none(),
                "{kind:?} survived class mutation"
            );
            assert!(
                cache
                    .get(kind, &7, CacheGuard::new(guard.loader, 11, 4))
                    .is_none(),
                "{kind:?} survived method mutation"
            );
            heap.release_root(owner_root).unwrap();
            let receipt = heap.collect().unwrap();
            assert!(
                !receipt.cleared_ephemerons.is_empty(),
                "{kind:?} lacked an exact ephemeron clearing receipt"
            );
            loader.test_remove("Owner");
            drop(owner);
            assert_eq!(
                cache.live_len(),
                0,
                "{kind:?} retained a dropped loader class"
            );
            heap.release_root(cache_root).unwrap();
        }
    }
}
