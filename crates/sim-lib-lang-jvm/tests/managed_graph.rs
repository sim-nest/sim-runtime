use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{JVM_ROLE_EDGE_TABLE, JvmEdge, JvmHeap, JvmRole};

fn limits() -> CollectionLimits {
    CollectionLimits {
        objects: 64,
        edges: 128,
        stack: 64,
        work: 1024,
        clears: 64,
        finalizers: 0,
    }
}

#[test]
fn edge_table_is_complete_over_the_role_enum() {
    let roles = [
        JvmRole::Object,
        JvmRole::Array,
        JvmRole::ClassMirror,
        JvmRole::Loader,
        JvmRole::Statics,
        JvmRole::String,
        JvmRole::Throwable,
        JvmRole::Monitor,
        JvmRole::PreparedMethod,
        JvmRole::Cache,
    ];
    assert_eq!(JVM_ROLE_EDGE_TABLE.len(), roles.len());
    for (ordinal, role) in roles.into_iter().enumerate() {
        assert_eq!(JVM_ROLE_EDGE_TABLE[ordinal].role, role);
    }
}

#[test]
fn unreachable_object_cycle_collects() {
    let mut heap = JvmHeap::new(8, limits()).unwrap();
    let first = heap.allocate(JvmRole::Object).unwrap();
    let second = heap.allocate(JvmRole::Object).unwrap();
    heap.strong(first, JvmEdge::Field, second).unwrap();
    heap.strong(second, JvmEdge::Field, first).unwrap();

    let receipt = heap.collect().unwrap();
    assert_eq!(receipt.swept, vec![first.id(), second.id()]);
    assert_eq!(heap.live_len(), 0);
}

#[test]
fn dropped_loader_reclaims_class_static_back_reference_cycle() {
    let mut heap = JvmHeap::new(8, limits()).unwrap();
    let loader = heap.allocate(JvmRole::Loader).unwrap();
    let class = heap.allocate(JvmRole::ClassMirror).unwrap();
    let statics = heap.allocate(JvmRole::Statics).unwrap();
    heap.strong(loader, JvmEdge::DefinedClass, class).unwrap();
    heap.strong(class, JvmEdge::DefiningLoader, loader).unwrap();
    heap.strong(class, JvmEdge::StaticStorage, statics).unwrap();
    heap.strong(statics, JvmEdge::StaticValue, loader).unwrap();

    let loader_root = heap.root(loader).unwrap();
    assert!(heap.collect().unwrap().swept.is_empty());
    heap.release_root(loader_root).unwrap();
    let receipt = heap.collect().unwrap();
    assert_eq!(receipt.swept, vec![loader.id(), class.id(), statics.id()]);
}

#[test]
fn dropped_class_key_clears_resolution_cache_exactly_once() {
    let mut heap = JvmHeap::new(8, limits()).unwrap();
    let cache = heap.allocate(JvmRole::Cache).unwrap();
    let class = heap.allocate(JvmRole::ClassMirror).unwrap();
    let method = heap.allocate(JvmRole::PreparedMethod).unwrap();
    let cache_root = heap.root(cache).unwrap();
    let entry = heap
        .ephemeron(cache, JvmEdge::DerivedEntry, class, method)
        .unwrap();

    let receipt = heap.collect().unwrap();
    assert_eq!(receipt.cleared_ephemerons, vec![(cache.id(), entry)]);
    assert_eq!(receipt.swept, vec![class.id(), method.id()]);
    assert!(heap.collect().unwrap().cleared_ephemerons.is_empty());
    heap.release_root(cache_root).unwrap();
}
