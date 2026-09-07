//! conformance: full M5 recovery, replay, observer, and fencing laws.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
        *self.cut.lock().expect("crash cut lock") = Some(CrashCut {
            admission,
            after_commit,
        });
    }
    fn clear(&self) {
        *self.cut.lock().expect("crash cut lock") = None;
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
        let cut = *self.cut.lock().expect("crash cut lock");
        if cut.is_some_and(|cut| cut.admission == index && !cut.after_commit) {
            return Err(JournalError::InjectedCrash("before M5 boundary"));
        }
        let result = self.inner.admit(admission);
        if result.is_ok() && cut.is_some_and(|cut| cut.admission == index && cut.after_commit) {
            return Err(JournalError::InjectedCrash("after M5 boundary"));
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

fn fixture(policy: ReplayPolicy) -> (OperationIntent, OperationGrant) {
    let intent = OperationIntent::new(
        "fixture/write",
        Datum::String("target/a".into()),
        Datum::String("present".into()),
        policy,
    )
    .expect("canonical fixture intent");
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("fixture/write"),
        Datum::String("authority/fixture".into()),
    )
    .expect("canonical fixture grant");
    (intent, grant)
}

struct FakePerformer {
    calls: Arc<AtomicUsize>,
    effect: Arc<AtomicBool>,
    acknowledge: bool,
}

impl LifecyclePerformer for FakePerformer {
    fn identity(&self) -> Datum {
        Datum::String("performer/fake".into())
    }
    fn perform(&mut self, dispatch: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.effect.store(true, Ordering::SeqCst);
        if self.acknowledge {
            LifecyclePerformerResponse::Receipt(Datum::String(dispatch.id().to_string()))
        } else {
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
}

struct EffectObserver {
    effect: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl PostconditionObserver for EffectObserver {
    fn identity(&self) -> Datum {
        Datum::String("observer/independent".into())
    }
    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.effect.load(Ordering::SeqCst) {
            PostconditionResponse::Satisfied {
                observed: Datum::String("present".into()),
                evidence: Datum::String("independent/read-present".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::String("absent".into()),
                evidence: Datum::String("independent/read-absent".into()),
            }
        }
    }
}

fn ports(
    acknowledge: bool,
) -> (
    FakePerformer,
    EffectObserver,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let effect = Arc::new(AtomicBool::new(false));
    let observations = Arc::new(AtomicUsize::new(0));
    (
        FakePerformer {
            calls: calls.clone(),
            effect: effect.clone(),
            acknowledge,
        },
        EffectObserver {
            effect: effect.clone(),
            calls: observations.clone(),
        },
        calls,
        effect,
        observations,
    )
}

#[test]
fn fresh_lifecycle_observes_before_dispatch_and_reconstructs_every_fact() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, calls, _, observations) = ports(true);
    let mut lifecycle = OperationLifecycle::from_shared(backend.clone());
    let outcome = lifecycle
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .expect("complete lifecycle");
    assert!(matches!(outcome, OperationOutcome::Verified { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observations.load(Ordering::SeqCst),
        2,
        "preflight observation precedes performance"
    );

    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .expect("verified journal")
        .expect("record");
    assert_eq!(record.leases().len(), 1);
    assert_eq!(record.attempts().len(), 1);
    assert_eq!(record.dispatches().len(), 1);
    assert_eq!(record.receipts().len(), 1);
    assert_eq!(record.observations().len(), 2);
    assert!(matches!(
        record.outcome(),
        Some(OperationOutcome::Verified { .. })
    ));
    assert_eq!(record.dispatches()[0].lease(), record.leases()[0].id());
}

#[test]
fn already_true_never_acquires_operation_lease_or_calls_performer() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, calls, effect, _) = ports(true);
    effect.store(true, Ordering::SeqCst);
    let outcome = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .expect("already true");
    assert!(matches!(outcome, OperationOutcome::AlreadyTrue { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert!(record.leases().is_empty() && record.dispatches().is_empty());
}

#[test]
fn exactly_once_missing_ack_observes_first_and_never_repeats_dispatch() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, calls, effect, _) = ports(false);
    let first = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(first, OperationOutcome::Verified { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    effect.store(false, Ordering::SeqCst);
    let mut reopened = OperationLifecycle::from_shared(backend.clone());
    let second = reopened
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/b".into()), 30, 40).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(
        matches!(second, OperationOutcome::Verified { .. }),
        "terminal verified evidence is stable"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        reopened
            .record(intent.id())
            .unwrap()
            .unwrap()
            .dispatches()
            .len(),
        1
    );
}

#[test]
fn lost_receipt_reconciles_from_external_state_without_reperformance() {
    let backend = Arc::new(CrashBackend::default());
    backend.crash(5, false);
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, calls, _, _) = ports(true);
    let error = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .expect_err("receipt persistence crashes");
    assert!(matches!(
        error,
        OperationError::Journal(JournalError::InjectedCrash(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    backend.clear();
    let outcome = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/b".into()), 30, 40).unwrap(),
            &mut performer,
            &mut observer,
        )
        .expect("independent reconciliation");
    assert!(matches!(outcome, OperationOutcome::Verified { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        OperationLifecycle::from_shared(backend)
            .record(intent.id())
            .unwrap()
            .unwrap()
            .receipts()
            .is_empty()
    );
}

#[test]
fn idempotent_retry_requires_expiry_and_observed_absence() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    let (mut performer, mut observer, calls, effect, _) = ports(false);
    let first = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(first, OperationOutcome::Verified { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Use a fresh operation so its first result remains retryable Diverged.
    let (intent, grant) = OperationIntent::new(
        "fixture/write/retry",
        Datum::String("target/b".into()),
        Datum::String("present".into()),
        ReplayPolicy::Idempotent,
    )
    .and_then(|intent| {
        OperationGrant::new(
            intent.id().clone(),
            CapabilityName::new("fixture/write"),
            Datum::String("authority/fixture".into()),
        )
        .map(|grant| (intent, grant))
    })
    .unwrap();
    effect.store(false, Ordering::SeqCst);
    struct NoEffect(Arc<AtomicUsize>);
    impl LifecyclePerformer for NoEffect {
        fn identity(&self) -> Datum {
            Datum::String("performer/no-effect".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            self.0.fetch_add(1, Ordering::SeqCst);
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
    let retry_calls = Arc::new(AtomicUsize::new(0));
    let mut no_effect = NoEffect(retry_calls.clone());
    let first = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 100, 120).unwrap(),
            &mut no_effect,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(first, OperationOutcome::Diverged { .. }));
    let before_expiry = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/b".into()), 110, 130).unwrap(),
            &mut no_effect,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(before_expiry, OperationOutcome::Uncertain { .. }));
    assert_eq!(retry_calls.load(Ordering::SeqCst), 1);
    let rewound_clock = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/rewound".into()), 90, 95).unwrap(),
            &mut no_effect,
            &mut observer,
        )
        .expect_err("a caller cannot move monotonic lease time backwards");
    assert!(matches!(rewound_clock, OperationError::InvalidLease));
    assert_eq!(retry_calls.load(Ordering::SeqCst), 1);
    let after_expiry = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/c".into()), 121, 140).unwrap(),
            &mut no_effect,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(after_expiry, OperationOutcome::Diverged { .. }));
    assert_eq!(retry_calls.load(Ordering::SeqCst), 2);
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.dispatches().len(), 2);
    assert!(record.leases()[1].fence() > record.leases()[0].fence());
}

#[test]
fn observer_disagreement_and_nonindependence_fail_closed() {
    struct Disputed;
    impl PostconditionObserver for Disputed {
        fn identity(&self) -> Datum {
            Datum::String("observer/disputed".into())
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            PostconditionResponse::Disputed {
                first: Datum::Bool(true),
                second: Datum::Bool(false),
            }
        }
    }
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, _, calls, _, _) = ports(true);
    let outcome = OperationLifecycle::new(CrashBackend::default())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 1, 2).unwrap(),
            &mut performer,
            &mut Disputed,
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    struct Same;
    impl PostconditionObserver for Same {
        fn identity(&self) -> Datum {
            Datum::String("performer/fake".into())
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            unreachable!()
        }
    }
    assert!(matches!(
        OperationLifecycle::new(CrashBackend::default()).run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 1, 2).unwrap(),
            &mut performer,
            &mut Same,
        ),
        Err(OperationError::ObserverNotIndependent)
    ));
}

#[test]
fn crashes_before_and_after_all_seven_durable_boundaries_preserve_truth() {
    // Seven journal admissions plus the performer call cover M5's eight
    // externally interruptible boundaries on the successful path.
    for admission in 1..=7 {
        for after_commit in [false, true] {
            let backend = Arc::new(CrashBackend::default());
            backend.crash(admission, after_commit);
            let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
            let (mut performer, mut observer, calls, effect, _) = ports(true);
            let result = OperationLifecycle::from_shared(backend.clone()).run(
                &intent,
                &grant,
                LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
                &mut performer,
                &mut observer,
            );
            assert!(matches!(
                result,
                Err(OperationError::Journal(JournalError::InjectedCrash(_)))
            ));
            let calls_after_crash = calls.load(Ordering::SeqCst);
            backend.clear();
            let recovered = OperationLifecycle::from_shared(backend.clone())
                .run(
                    &intent,
                    &grant,
                    LeaseWindow::new(Datum::String("holder/b".into()), 30, 40).unwrap(),
                    &mut performer,
                    &mut observer,
                )
                .expect("recovery reaches truthful outcome");
            assert!(matches!(
                recovered,
                OperationOutcome::Verified { .. } | OperationOutcome::Diverged { .. }
            ));
            assert!(
                calls.load(Ordering::SeqCst) <= 1,
                "ExactlyOnce repeated after admission {admission}/{after_commit}"
            );
            if calls_after_crash == 1 {
                assert!(effect.load(Ordering::SeqCst));
            }
        }
    }
}

#[test]
fn bounded_lease_rejects_zero_duration() {
    assert!(matches!(
        LeaseWindow::new(Datum::String("holder/a".into()), 10, 10),
        Err(OperationError::InvalidLease)
    ));
}

#[test]
fn last_persisted_kind_is_canonical_operation_symbol() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, _, effect, _) = ports(true);
    effect.store(true, Ordering::SeqCst);
    OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 1, 2).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert_eq!(
        backend
            .read_state()
            .unwrap()
            .entries
            .last_key_value()
            .unwrap()
            .1
            .kind,
        Symbol::qualified("operation", "lifecycle-outcome-persisted")
    );
}
