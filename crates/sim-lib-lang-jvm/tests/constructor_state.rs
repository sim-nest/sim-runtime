use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{
    ConstructorState, InvocationKind, JvmHeap, JvmReference, JvmRole, UninitializedUse,
    VerificationFidelity,
};
use sim_lib_machine::SourceLocation;

fn location(source: &str, start: usize) -> SourceLocation {
    SourceLocation::Bytes(Origin {
        codec: CodecId(0),
        source: SourceId(source.into()),
        span: Span {
            start,
            end: start + 1,
        },
        trivia: Vec::new(),
    })
}

fn heap() -> JvmHeap {
    JvmHeap::new(
        8,
        CollectionLimits {
            objects: 8,
            edges: 8,
            stack: 8,
            work: 64,
            clears: 8,
            finalizers: 0,
        },
    )
    .unwrap()
}

#[test]
fn alias_use_is_refused_with_allocation_site_and_static_checked_fidelity() {
    let mut heap = heap();
    let handle = heap.allocate(JvmRole::Object).unwrap();
    let allocation = location("Example.<init>", 7);
    let use_site = location("Example.<init>", 12);
    let mut state = ConstructorState::default();
    state.allocated(handle, allocation.clone());

    let alias = JvmReference::managed(handle);
    let error = state
        .require_initialized(alias, use_site.clone())
        .unwrap_err();
    assert_eq!(error.allocation_location(), &allocation);
    assert_eq!(error.use_location(), &use_site);
    assert_eq!(error.attempted_use(), UninitializedUse::Ordinary);
    assert!(error.to_string().contains("earlier admitted effects"));
    assert_eq!(state.fidelity(), VerificationFidelity::StaticChecked);

    state
        .invoke(alias, InvocationKind::Special, "<init>", use_site)
        .unwrap();
    state
        .require_initialized(alias, location("Example.<init>", 15))
        .unwrap();
}

#[test]
fn constructor_invocation_and_return_rules_are_enforced() {
    let mut heap = heap();
    let receiver = heap.allocate(JvmRole::Object).unwrap();
    let reference = JvmReference::managed(receiver);
    let entry = location("Example.<init>", 0);
    let mut state = ConstructorState::default();
    state.constructor_entry(receiver, entry.clone());

    assert_eq!(
        state
            .invoke(
                reference,
                InvocationKind::Virtual,
                "<init>",
                location("Example.<init>", 2),
            )
            .unwrap_err()
            .attempted_use(),
        UninitializedUse::ConstructorInvocation
    );
    assert_eq!(
        state
            .constructor_return(location("Example.<init>", 3))
            .unwrap_err()
            .attempted_use(),
        UninitializedUse::ConstructorReturn
    );
    state
        .invoke(
            reference,
            InvocationKind::Special,
            "<init>",
            location("Example.<init>", 4),
        )
        .unwrap();
    state
        .constructor_return(location("Example.<init>", 5))
        .unwrap();
}

#[test]
fn verifier_strengthens_the_same_state_path() {
    let state = ConstructorState::default();
    let verified = state.strengthen("proof");
    assert!(std::ptr::eq(verified.state(), &state));
    assert_eq!(verified.proof(), &"proof");
    assert_eq!(verified.fidelity(), VerificationFidelity::Verified);
}
