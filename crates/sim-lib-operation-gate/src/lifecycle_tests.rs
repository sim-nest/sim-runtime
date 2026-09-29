//! conformance: full M5 recovery, replay, observer, and fencing laws.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use sim_kernel::{CapabilityName, ContentId, Datum, Symbol};
use sim_lib_journal::{
    Admission, Journal, JournalBackend, JournalEntry, JournalError, JournalHead, JournalObject,
    MemoryBackend, StoredDatumRef, StoredState,
};

use super::*;
mod acceptance_custody;
mod divergence;
mod preparation_tests;
mod verified_record;

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
    inside_release: AtomicBool,
    before_admission: Mutex<Option<BeforeAdmission>>,
}

struct BeforeAdmission {
    kind: Symbol,
    action: Box<dyn FnOnce() + Send>,
}

impl CrashBackend {
    fn before_admission(&self, admission: &Admission) {
        let hook = {
            let mut slot = self.before_admission.lock().unwrap();
            if slot.as_ref().is_some_and(|hook| {
                admission
                    .entries()
                    .last()
                    .is_some_and(|entry| entry.kind == hook.kind)
            }) {
                slot.take()
            } else {
                None
            }
        };
        if let Some(hook) = hook {
            (hook.action)();
        }
    }
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
        assert!(
            !self.inside_release.load(Ordering::SeqCst),
            "release action reentered its journal"
        );
        self.inner.read_state()
    }
    fn admit(&self, admission: Admission) -> Result<JournalHead, JournalError> {
        self.before_admission(&admission);
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
    fn admit_then(
        &self,
        admission: Admission,
        action: &mut dyn sim_lib_journal::CommitAction,
    ) -> Result<sim_lib_journal::GuardedAdmission, JournalError> {
        self.before_admission(&admission);
        let index = self.admissions.fetch_add(1, Ordering::SeqCst) + 1;
        let cut = *self.cut.lock().expect("crash cut lock");
        if cut.is_some_and(|cut| cut.admission == index && !cut.after_commit) {
            return Err(JournalError::InjectedCrash("before release boundary"));
        }
        let result = self.inner.admit_then(admission, &mut || {
            assert!(!self.inside_release.swap(true, Ordering::SeqCst));
            action.after_commit();
            self.inside_release.store(false, Ordering::SeqCst);
        });
        if result.is_ok() && cut.is_some_and(|cut| cut.admission == index && cut.after_commit) {
            return Err(JournalError::InjectedCrash("after release boundary"));
        }
        result
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
fn public_snapshot_projection_accepts_exact_complete_and_prospective_cut() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, _, _, _) = ports(true);
    OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();

    let full = Journal::new(backend).verified_snapshot().unwrap();
    let projected = project_verified_lifecycle_records(&full).unwrap();
    assert!(matches!(
        projected
            .get(intent.id())
            .and_then(OperationLifecycleRecord::outcome),
        Some(OperationOutcome::Verified { .. })
    ));

    let prefix_journal = Journal::new(MemoryBackend::new());
    let lease = prefix_journal.acquire_lease().unwrap();
    let cut_index = full.entries().len() - 1;
    let prefix_entries = full.entries()[..cut_index].to_vec();
    let prefix_ids = prefix_entries
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let prefix_objects = prefix_ids
        .into_iter()
        .map(|id| JournalObject::from_datum(full.datum(id).unwrap().clone()).unwrap())
        .collect();
    prefix_journal
        .publish(&lease, None, prefix_objects, prefix_entries)
        .unwrap();
    let prefix = prefix_journal.verified_snapshot().unwrap();
    let cut = &full.entries()[cut_index];
    let cut_objects = cut
        .payloads
        .iter()
        .map(|id| JournalObject::from_datum(full.datum(id).unwrap().clone()).unwrap())
        .collect::<Vec<_>>();
    let projected = project_verified_lifecycle_extension(&prefix, cut, &cut_objects).unwrap();
    let record = projected.get(intent.id()).unwrap();
    assert!(matches!(
        record.outcome(),
        Some(OperationOutcome::Verified { .. })
    ));

    let wrong_sequence = JournalEntry::new(
        cut.sequence + 1,
        cut.previous.clone(),
        cut.kind.clone(),
        cut.payloads.clone(),
    );
    assert!(project_verified_lifecycle_extension(&prefix, &wrong_sequence, &cut_objects).is_err());
    let unknown = JournalEntry::new(
        cut.sequence,
        cut.previous.clone(),
        Symbol::qualified("operation", "foreign-transition"),
        cut.payloads.clone(),
    );
    assert!(project_verified_lifecycle_extension(&prefix, &unknown, &cut_objects).is_err());
    let extra = JournalObject::from_datum(Datum::String("unbound".into())).unwrap();
    let mut extra_objects = cut_objects;
    extra_objects.push(extra);
    assert!(project_verified_lifecycle_extension(&prefix, cut, &extra_objects).is_err());

    let foreign_journal = Journal::new(MemoryBackend::new());
    let foreign_lease = foreign_journal.acquire_lease().unwrap();
    let foreign_object = JournalObject::from_datum(Datum::String("foreign".into())).unwrap();
    let foreign_entry = JournalEntry::new(
        0,
        None,
        Symbol::qualified("operation", "foreign-transition"),
        vec![foreign_object.id.clone()],
    );
    foreign_journal
        .publish(
            &foreign_lease,
            None,
            vec![foreign_object],
            vec![foreign_entry],
        )
        .unwrap();
    assert!(
        project_verified_lifecycle_records(&foreign_journal.verified_snapshot().unwrap()).is_err()
    );
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
