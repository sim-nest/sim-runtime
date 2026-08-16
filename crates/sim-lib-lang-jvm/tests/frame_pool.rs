use sim_lib_control::WorkLimit;
use sim_lib_gc_tracing::{CollectionLimits, ManagedHeap};
use sim_lib_lang_jvm::{JvmFramePool, JvmFramePoolPolicy, JvmReference, JvmValue};
use sim_lib_machine::RootSnapshot;
use sim_lib_mutation::ManagedNode;

fn pool(frames: usize) -> JvmFramePool {
    JvmFramePool::new(JvmFramePoolPolicy {
        frames,
        slots: 8,
        operands: 8,
    })
}

fn collection_limits() -> CollectionLimits {
    CollectionLimits {
        objects: 8,
        edges: 8,
        stack: 8,
        work: 64,
        clears: 8,
        finalizers: 0,
    }
}

#[test]
fn deep_recursive_return_clears_every_pooled_root_before_collection() {
    let pool = pool(128);
    let mut heap = ManagedHeap::tracing(8, collection_limits()).unwrap();
    let dead = heap.allocate(ManagedNode::new(())).unwrap();
    let mut active = Vec::new();

    for _ in 0..128 {
        let mut frame = pool.acquire(2, 2);
        frame
            .frame_mut()
            .locals_mut()
            .store(0, JvmValue::Reference(JvmReference::managed(dead)))
            .unwrap();
        frame
            .frame_mut()
            .operands_mut()
            .push(JvmValue::Reference(JvmReference::managed(dead)))
            .unwrap();
        active.push(frame);
    }

    assert_eq!(
        pool.retained_frames(),
        0,
        "live recursive frames are exclusive"
    );
    while let Some(frame) = active.pop() {
        frame.complete();
    }
    assert_eq!(pool.retained_frames(), 128);
    assert!(
        RootSnapshot::scan(&pool, WorkLimit(0))
            .unwrap()
            .roots()
            .is_empty(),
        "the full pool enumerator must observe no dead reference"
    );

    let receipt = heap.collect().unwrap().unwrap();
    assert_eq!(receipt.swept, [dead.id()]);
}

#[test]
fn interruption_retains_exclusive_live_frame_until_resume() {
    let pool = pool(1);
    let mut heap = ManagedHeap::tracing(2, collection_limits()).unwrap();
    let live = heap.allocate(ManagedNode::new(())).unwrap();
    let mut frame = pool.acquire(1, 1);
    frame
        .frame_mut()
        .operands_mut()
        .push(JvmValue::Reference(JvmReference::managed(live)))
        .unwrap();

    let interrupted = frame.interrupt();
    assert_eq!(pool.retained_frames(), 0);
    assert_eq!(
        RootSnapshot::scan(&interrupted, WorkLimit(1))
            .unwrap()
            .roots(),
        [live.id()]
    );

    interrupted.resume().complete();
    assert_eq!(pool.retained_frames(), 1);
    assert!(RootSnapshot::scan(&pool, WorkLimit(0)).is_ok());
}

#[test]
fn retained_record_count_and_capacity_are_both_capped() {
    let pool = pool(1);
    pool.acquire(1, 1).complete();
    pool.acquire(2, 2).complete();
    assert_eq!(pool.retained_frames(), 1);

    let oversized = pool.acquire(9, 9);
    oversized.complete();
    assert_eq!(pool.retained_frames(), 1);
}
