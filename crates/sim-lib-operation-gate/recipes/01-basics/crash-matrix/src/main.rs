use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use sim_kernel::{CapabilityName, Datum};
use sim_lib_journal::MemoryBackend;
use sim_lib_operation_gate::{
    DurableOperationState, OperationAttempt, OperationDispatch, OperationGrant, OperationIntent,
    OperationPerformer, OperationService, PerformerResponse, ReplayPolicy,
};

struct MissingAcknowledgement(Arc<AtomicUsize>);

impl OperationPerformer for MissingAcknowledgement {
    fn perform(&mut self, _: &OperationDispatch) -> PerformerResponse {
        self.0.fetch_add(1, Ordering::SeqCst);
        PerformerResponse::AcknowledgementMissing
    }
}

struct MustNotRun(Arc<AtomicUsize>);

impl OperationPerformer for MustNotRun {
    fn perform(&mut self, _: &OperationDispatch) -> PerformerResponse {
        self.0.fetch_add(1, Ordering::SeqCst);
        PerformerResponse::Receipt(Datum::String("unexpected".into()))
    }
}

fn main() {
    let backend = Arc::new(MemoryBackend::new());
    let intent = OperationIntent::new(
        "fixture/write",
        Datum::String("target/a".into()),
        Datum::String("present".into()),
        ReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    let same_intent = OperationIntent::new(
        "fixture/write",
        Datum::String("target/a".into()),
        Datum::String("present".into()),
        ReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("fixture/write"),
        Datum::String("approval/a".into()),
    )
    .unwrap();
    let first_attempt = OperationAttempt::new(intent.id().clone(), 0).unwrap();
    let invocations = Arc::new(AtomicUsize::new(0));
    let mut service = OperationService::from_shared(backend.clone());
    service.resume().unwrap();
    let first = service
        .execute(
            &intent,
            &grant,
            &first_attempt,
            &mut MissingAcknowledgement(invocations.clone()),
        )
        .unwrap();

    let mut reopened = OperationService::from_shared(backend);
    reopened.resume().unwrap();
    let replay_attempt = OperationAttempt::new(intent.id().clone(), 1).unwrap();
    let replay = reopened
        .execute(
            &intent,
            &grant,
            &replay_attempt,
            &mut MustNotRun(invocations.clone()),
        )
        .unwrap();

    println!("first state: {}", state_name(&first),);
    println!("reopened state: {}", state_name(&replay));
    println!(
        "performer invocations: {}",
        invocations.load(Ordering::SeqCst)
    );
    println!(
        "operation identity stable: {}",
        intent.id() == same_intent.id()
    );
    println!("real effects: 0");
}

fn state_name(state: &DurableOperationState) -> &'static str {
    match state {
        DurableOperationState::IntentPersisted { .. } => "intent",
        DurableOperationState::Dispatched { .. } => "dispatched",
        DurableOperationState::ReceiptPersisted { .. } => "receipt",
    }
}
