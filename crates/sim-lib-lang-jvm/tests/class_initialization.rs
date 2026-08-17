use std::sync::Arc;

use sim_kernel::{CodecId, Cx, DefaultFactory, NoopEvalPolicy, Origin, SourceId, Span, Symbol};
use sim_lib_control::Raised;
use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{
    ClassInitialization, ClassInitializationState, InitializationAction, InitializationLane,
    InitializationPlan, InitializationResume, JvmHeap, JvmRole,
};

fn raised(label: &str) -> Raised {
    let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    Raised::new(
        cx.factory()
            .symbol(Symbol::new("java/lang/Throwable"))
            .unwrap(),
        cx.factory().string(label.into()).unwrap(),
        Origin {
            codec: CodecId(0),
            source: SourceId("class-initialization".into()),
            span: Span { start: 0, end: 1 },
            trivia: vec![],
        },
        Symbol::qualified("lang", "jvm"),
    )
    .unwrap()
}

fn fixture() -> (JvmHeap, ClassInitialization) {
    let mut heap = JvmHeap::new(
        16,
        CollectionLimits {
            objects: 16,
            edges: 64,
            stack: 16,
            work: 128,
            clears: 16,
            finalizers: 0,
        },
    )
    .unwrap();
    let mirror = heap.allocate(JvmRole::ClassMirror).unwrap();
    let plan = InitializationPlan::new(
        "Child",
        Some("Parent"),
        ["LeftDefault", "RightDefault"],
        true,
        5,
    )
    .unwrap();
    (heap, ClassInitialization::new(plan, mirror))
}

#[test]
fn actions_resume_in_jvms_order_and_same_lane_reentrancy_proceeds() {
    let (_heap, mut initialization) = fixture();
    let lane = InitializationLane(7);
    let expected = [
        InitializationAction::InstallStaticConstants,
        InitializationAction::InitializeSuperclass(Arc::from("Parent")),
        InitializationAction::InitializeSuperinterface(Arc::from("LeftDefault")),
        InitializationAction::InitializeSuperinterface(Arc::from("RightDefault")),
        InitializationAction::InvokeClassInitializer,
    ];
    for action in expected {
        assert_eq!(
            initialization.resume(lane).unwrap(),
            InitializationResume::Action(action)
        );
        assert_eq!(
            initialization.resume(lane).unwrap(),
            InitializationResume::Reentrant
        );
        initialization.complete_action(lane).unwrap();
    }
    assert_eq!(
        initialization.resume(lane).unwrap(),
        InitializationResume::Initialized
    );
    assert_eq!(
        initialization.snapshot().state,
        ClassInitializationState::Initialized
    );
}

#[test]
fn initializer_failure_is_stable_and_its_managed_cause_is_collectible() {
    let (mut heap, mut initialization) = fixture();
    let lane = InitializationLane(9);
    assert!(matches!(
        initialization.resume(lane),
        Ok(InitializationResume::Action(_))
    ));
    let cause = heap.allocate(JvmRole::Throwable).unwrap();
    let original = raised("original");
    let wrapped = initialization
        .fail(&mut heap, lane, cause, &original, |_| {
            raised("ExceptionInInitializerError")
        })
        .unwrap();
    assert_eq!(
        initialization.resume(lane).unwrap(),
        InitializationResume::Erroneous(wrapped.clone())
    );
    assert_eq!(
        initialization.resume(InitializationLane(10)).unwrap(),
        InitializationResume::Erroneous(wrapped)
    );
    assert!(!heap.collect().unwrap().swept.contains(&cause.id()));
    initialization.release_failure(&mut heap).unwrap();
    let swept = heap.collect().unwrap().swept;
    assert!(swept.contains(&cause.id()));
    assert_eq!(
        initialization.snapshot().state,
        ClassInitializationState::Erroneous
    );
}
