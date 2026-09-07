use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use sim_kernel::{CapabilityName, ContentId, Datum, Symbol};
use sim_lib_journal::{
    Admission, JournalBackend, JournalError, JournalHead, JournalObject, MemoryBackend,
    StoredDatumRef, StoredState,
};

use super::*;

#[derive(Clone, Copy)]
struct CrashCut {
    admission: usize,
    after_commit: bool,
}

#[derive(Default)]
struct CrashBackend {
    inner: MemoryBackend,
    admissions: AtomicUsize,
    cut: Mutex<Option<CrashCut>>,
}

impl CrashBackend {
    fn crash(&self, admission: usize, after_commit: bool) {
        self.admissions.store(0, Ordering::SeqCst);
        *self.cut.lock().unwrap() = Some(CrashCut {
            admission,
            after_commit,
        });
    }

    fn clear_crash(&self) {
        *self.cut.lock().unwrap() = None;
    }
}

impl JournalBackend for CrashBackend {
    fn acquire_lease(&self) -> Result<sim_lib_journal::Lease, JournalError> {
        self.inner.acquire_lease()
    }

    fn read_state(&self) -> Result<StoredState, JournalError> {
        self.inner.read_state()
    }

    fn admit(&self, admission: Admission) -> Result<JournalHead, JournalError> {
        let index = self.admissions.fetch_add(1, Ordering::SeqCst) + 1;
        let cut = *self.cut.lock().unwrap();
        if cut.is_some_and(|cut| cut.admission == index && !cut.after_commit) {
            return Err(JournalError::InjectedCrash("before operation boundary"));
        }
        let result = self.inner.admit(admission);
        if result.is_ok() && cut.is_some_and(|cut| cut.admission == index && cut.after_commit) {
            return Err(JournalError::InjectedCrash("after operation boundary"));
        }
        result
    }

    fn put_datum(&self, object: JournalObject) -> Result<StoredDatumRef, JournalError> {
        self.inner.put_datum(object)
    }

    fn get_datum(&self, meaning: &ContentId) -> Result<Datum, JournalError> {
        self.inner.get_datum(meaning)
    }

    fn rebuild_datum_index(&self) -> Result<Vec<StoredDatumRef>, JournalError> {
        self.inner.rebuild_datum_index()
    }
}

struct FakePerformer {
    invocations: Arc<AtomicUsize>,
    response: PerformerResponse,
    backend: Option<Arc<CrashBackend>>,
}

impl FakePerformer {
    fn receipt(invocations: Arc<AtomicUsize>) -> Self {
        Self {
            invocations,
            response: PerformerResponse::Receipt(Datum::String("raw-ok".into())),
            backend: None,
        }
    }

    fn missing(invocations: Arc<AtomicUsize>) -> Self {
        Self {
            invocations,
            response: PerformerResponse::AcknowledgementMissing,
            backend: None,
        }
    }
}

impl OperationPerformer for FakePerformer {
    fn perform(&mut self, _: &OperationDispatch) -> PerformerResponse {
        if let Some(backend) = &self.backend {
            let state = backend.read_state().unwrap();
            assert_eq!(
                state.entries.last_key_value().unwrap().1.kind,
                Symbol::qualified("operation", "dispatched"),
                "performer ran before durable dispatch"
            );
        }
        self.invocations.fetch_add(1, Ordering::SeqCst);
        self.response.clone()
    }
}

fn intent(policy: ReplayPolicy) -> OperationIntent {
    OperationIntent::new(
        "fixture/write",
        Datum::String("target/a".into()),
        Datum::String("present".into()),
        policy,
    )
    .unwrap()
}

fn grant(intent: &OperationIntent, authority: &str) -> OperationGrant {
    OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("fixture/write"),
        Datum::String(authority.into()),
    )
    .unwrap()
}

#[test]
fn complete_execution_reconstructs_canonical_intent_dispatch_and_raw_receipt() {
    let backend = Arc::new(CrashBackend::default());
    let intent = intent(ReplayPolicy::ExactlyOnce);
    let intent_grant = grant(&intent, "approval/a");
    let attempt = OperationAttempt::new(intent.id().clone(), 0).unwrap();
    let invocations = Arc::new(AtomicUsize::new(0));
    let mut performer = FakePerformer::receipt(invocations.clone());
    performer.backend = Some(backend.clone());
    let mut service = OperationService::from_shared(backend.clone());
    service.resume().unwrap();

    let state = service
        .execute(&intent, &intent_grant, &attempt, &mut performer)
        .unwrap();
    assert!(matches!(
        state,
        DurableOperationState::ReceiptPersisted { .. }
    ));
    assert_eq!(invocations.load(Ordering::SeqCst), 1);

    let reopened = OperationService::from_shared(backend);
    let record = reopened.record(intent.id()).unwrap().unwrap();
    assert_eq!(record.intent(), &intent);
    assert_eq!(record.grant(), &intent_grant);
    assert_eq!(record.attempt(), Some(&attempt));
    assert_eq!(
        record.receipt().unwrap().raw(),
        &Datum::String("raw-ok".into())
    );
    assert!(matches!(
        record.state(),
        DurableOperationState::ReceiptPersisted { .. }
    ));
}

#[test]
fn every_log_boundary_crash_reconstructs_and_recovery_never_repeats_dispatch() {
    for (cut, expected_state, expected_invocations) in [
        (
            CrashCut {
                admission: 1,
                after_commit: false,
            },
            "absent",
            0,
        ),
        (
            CrashCut {
                admission: 1,
                after_commit: true,
            },
            "intent",
            0,
        ),
        (
            CrashCut {
                admission: 2,
                after_commit: false,
            },
            "intent",
            0,
        ),
        (
            CrashCut {
                admission: 2,
                after_commit: true,
            },
            "dispatch",
            0,
        ),
        (
            CrashCut {
                admission: 3,
                after_commit: false,
            },
            "dispatch",
            1,
        ),
        (
            CrashCut {
                admission: 3,
                after_commit: true,
            },
            "receipt",
            1,
        ),
    ] {
        let backend = Arc::new(CrashBackend::default());
        backend.crash(cut.admission, cut.after_commit);
        let intent = intent(ReplayPolicy::ExactlyOnce);
        let grant = grant(&intent, "approval/a");
        let attempt = OperationAttempt::new(intent.id().clone(), 0).unwrap();
        let invocations = Arc::new(AtomicUsize::new(0));
        let mut performer = FakePerformer::receipt(invocations.clone());
        let mut service = OperationService::from_shared(backend.clone());
        service.resume().unwrap();
        assert!(matches!(
            service.execute(&intent, &grant, &attempt, &mut performer),
            Err(OperationError::Journal(JournalError::InjectedCrash(_)))
        ));

        backend.clear_crash();
        let count_after_crash = invocations.load(Ordering::SeqCst);
        let durable_after_crash = OperationService::from_shared(backend.clone())
            .state(intent.id())
            .unwrap();
        assert_eq!(state_name(durable_after_crash.as_ref()), expected_state);
        assert_eq!(count_after_crash, expected_invocations);
        let mut recovered = OperationService::from_shared(backend);
        recovered.resume().unwrap();
        let later_attempt = OperationAttempt::new(intent.id().clone(), 99).unwrap();
        let mut recovery_performer = FakePerformer::receipt(invocations.clone());
        let recovered_state = recovered
            .execute(&intent, &grant, &later_attempt, &mut recovery_performer)
            .unwrap();

        if matches!(
            durable_after_crash,
            Some(DurableOperationState::Dispatched { .. })
                | Some(DurableOperationState::ReceiptPersisted { .. })
        ) {
            assert_eq!(invocations.load(Ordering::SeqCst), count_after_crash);
        } else {
            assert_eq!(invocations.load(Ordering::SeqCst), count_after_crash + 1);
        }
        assert!(matches!(
            recovered_state,
            DurableOperationState::Dispatched { .. }
                | DurableOperationState::ReceiptPersisted { .. }
        ));
    }
}

#[test]
fn missing_acknowledgement_is_durable_uncertainty_without_retry_or_failure() {
    let backend = Arc::new(CrashBackend::default());
    let intent = intent(ReplayPolicy::ExactlyOnce);
    let first_grant = grant(&intent, "approval/a");
    let attempt = OperationAttempt::new(intent.id().clone(), 0).unwrap();
    let invocations = Arc::new(AtomicUsize::new(0));
    let mut performer = FakePerformer::missing(invocations.clone());
    let mut service = OperationService::from_shared(backend.clone());
    service.resume().unwrap();
    assert!(matches!(
        service
            .execute(&intent, &first_grant, &attempt, &mut performer)
            .unwrap(),
        DurableOperationState::Dispatched { .. }
    ));

    let second_grant = grant(&intent, "approval/b");
    let second_attempt = OperationAttempt::new(intent.id().clone(), 1).unwrap();
    let mut replay_performer = FakePerformer::receipt(invocations.clone());
    let mut reopened = OperationService::from_shared(backend);
    reopened.resume().unwrap();
    assert!(matches!(
        reopened
            .execute(
                &intent,
                &second_grant,
                &second_attempt,
                &mut replay_performer,
            )
            .unwrap(),
        DurableOperationState::Dispatched { .. }
    ));
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert!(
        reopened
            .record(intent.id())
            .unwrap()
            .unwrap()
            .receipt()
            .is_none()
    );
}

#[test]
fn operation_identity_binds_semantics_but_excludes_grants_attempts_and_leases() {
    let base = intent(ReplayPolicy::ExactlyOnce);
    assert_eq!(base.id(), intent(ReplayPolicy::ExactlyOnce).id());
    assert_ne!(base.id(), intent(ReplayPolicy::Idempotent).id());
    assert_ne!(
        base.id(),
        OperationIntent::new(
            "fixture/write",
            Datum::String("target/b".into()),
            Datum::String("present".into()),
            ReplayPolicy::ExactlyOnce,
        )
        .unwrap()
        .id()
    );
    assert_ne!(
        base.id(),
        OperationIntent::new(
            "fixture/write",
            Datum::String("target/a".into()),
            Datum::String("absent".into()),
            ReplayPolicy::ExactlyOnce,
        )
        .unwrap()
        .id()
    );
    let grant_a = grant(&base, "approval/a");
    let grant_b = grant(&base, "approval/b");
    assert_ne!(grant_a.id(), grant_b.id());
    let attempt_zero = OperationAttempt::new(base.id().clone(), 0).unwrap();
    let attempt_one = OperationAttempt::new(base.id().clone(), 1).unwrap();
    assert_ne!(attempt_zero.id(), attempt_one.id());
    let dispatch_before = OperationDispatch::new(
        base.id().clone(),
        grant_a.id().clone(),
        attempt_zero.id().clone(),
    )
    .unwrap();

    let backend = Arc::new(CrashBackend::default());
    let mut first = OperationService::from_shared(backend.clone());
    first.resume().unwrap();
    let mut second = OperationService::from_shared(backend);
    second.resume().unwrap();
    assert_eq!(base.id(), intent(ReplayPolicy::ExactlyOnce).id());
    let dispatch_after = OperationDispatch::new(
        base.id().clone(),
        grant_a.id().clone(),
        attempt_zero.id().clone(),
    )
    .unwrap();
    assert_eq!(dispatch_before.id(), dispatch_after.id());
}

fn state_name(state: Option<&DurableOperationState>) -> &'static str {
    match state {
        None => "absent",
        Some(DurableOperationState::IntentPersisted { .. }) => "intent",
        Some(DurableOperationState::Dispatched { .. }) => "dispatch",
        Some(DurableOperationState::ReceiptPersisted { .. }) => "receipt",
    }
}

#[test]
fn contradictory_claimed_intent_and_cross_operation_authority_fail_closed() {
    let intent = intent(ReplayPolicy::ExactlyOnce);
    let intent_grant = grant(&intent, "approval/a");
    let attempt = OperationAttempt::new(intent.id().clone(), 0).unwrap();
    let mut contradictory = intent.clone();
    contradictory.target = Datum::String("target/substituted".into());
    let mut service = OperationService::new(CrashBackend::default());
    service.resume().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    assert!(matches!(
        service.execute(
            &contradictory,
            &intent_grant,
            &attempt,
            &mut FakePerformer::receipt(count.clone()),
        ),
        Err(OperationError::ContradictoryIntent)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);

    let other = OperationIntent::new(
        "fixture/other",
        Datum::Nil,
        Datum::Nil,
        ReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    assert!(matches!(
        service.execute(
            &intent,
            &grant(&other, "approval/other"),
            &attempt,
            &mut FakePerformer::receipt(count),
        ),
        Err(OperationError::GrantMismatch)
    ));
}

#[test]
fn durable_owner_contains_no_process_or_network_performer() {
    let sources = concat!(
        include_str!("durable.rs"),
        include_str!("operation_service.rs"),
        include_str!("operation_wire.rs"),
    );
    assert!(!sources.contains("std::process"));
    assert!(!sources.contains("std::net"));
    assert!(!sources.contains("Command::new"));
    assert!(!sources.contains("TcpStream"));
}
