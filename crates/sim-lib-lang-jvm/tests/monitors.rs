use sim_lib_control::{CleanupStack, RaisedUnwind};
use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{FailureCondition, JvmHeap, JvmRole, MonitorLane, MonitorTable};

fn limits() -> CollectionLimits {
    CollectionLimits {
        objects: 8,
        edges: 8,
        stack: 8,
        work: 32,
        clears: 8,
        finalizers: 8,
    }
}

#[test]
fn unwind_releases_three_nested_monitors_in_reverse_order() {
    let mut heap = JvmHeap::new(8, limits()).unwrap();
    let objects = [
        heap.allocate(JvmRole::Object).unwrap(),
        heap.allocate(JvmRole::Object).unwrap(),
        heap.allocate(JvmRole::Object).unwrap(),
    ];
    let monitors = MonitorTable::new();
    let lane = MonitorLane(0);
    let mut cleanups: CleanupStack<RaisedUnwind<(), (), ()>> = CleanupStack::new();
    for object in objects {
        monitors.enter(lane, object, &mut cleanups);
    }

    let _ = cleanups.unwind(RaisedUnwind::Cancelled);

    assert_eq!(
        monitors.release_order(),
        objects.into_iter().rev().collect::<Vec<_>>()
    );
    assert!(
        objects
            .into_iter()
            .all(|object| monitors.recursion(object) == 0)
    );
}

#[test]
fn monitor_is_reentrant_and_normal_exit_cancels_its_unwind_release() {
    let mut heap = JvmHeap::new(2, limits()).unwrap();
    let object = heap.allocate(JvmRole::Object).unwrap();
    let monitors = MonitorTable::new();
    let lane = MonitorLane(0);
    let mut cleanups: CleanupStack<RaisedUnwind<(), (), ()>> = CleanupStack::new();
    monitors.enter(lane, object, &mut cleanups);
    monitors.enter(lane, object, &mut cleanups);
    assert_eq!(monitors.recursion(object), 2);
    monitors.exit(lane, object).unwrap();
    assert_eq!(monitors.recursion(object), 1);
    let _ = cleanups.unwind(RaisedUnwind::Closed);
    assert_eq!(monitors.recursion(object), 0);
    assert_eq!(monitors.release_order(), vec![object, object]);
}

#[test]
fn unbalanced_exit_names_java_illegal_monitor_state() {
    let mut heap = JvmHeap::new(2, limits()).unwrap();
    let object = heap.allocate(JvmRole::Object).unwrap();
    let error = MonitorTable::new()
        .exit(MonitorLane(0), object)
        .unwrap_err();
    assert_eq!(error.condition(), FailureCondition::IllegalMonitorState);
    assert_eq!(
        error.condition().java_class(),
        Some("java/lang/IllegalMonitorStateException")
    );
}
