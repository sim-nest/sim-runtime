//! Retained ownership across admission and cancellation-restoration cuts.

use super::*;

fn window(holder: &str) -> LeaseWindow {
    LeaseWindow::new(Datum::String(holder.into()), 10, 20).unwrap()
}

#[test]
fn admission_acknowledgement_loss_retains_exact_owner_without_retry() {
    for after_commit in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let lifecycle = OperationLifecycle::from_shared(backend.clone());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        backend.crash(1, after_commit);

        let failure = match lifecycle.accept_retaining(&intent, &grant, window("holder/a")) {
            Err(OperationAcceptanceError::Retained(failure)) => failure,
            Err(OperationAcceptanceError::Refused(error)) => {
                panic!("admission cut lost its owner: {error:?}")
            }
            Ok(_) => panic!("admission cut unexpectedly succeeded"),
        };
        assert_eq!(
            failure.stage(),
            OperationAcceptanceStage::AdmissionPublicationUncertain
        );
        assert_eq!(failure.operation_id(), intent.id());
        assert_eq!(failure.grant_id(), grant.id());

        backend.clear();
        let before = backend.read_state().unwrap().head;
        let verified = failure.revalidate().unwrap();
        assert_eq!(
            verified.journal_head(),
            before.as_ref().map(|head| &head.entry)
        );
        assert_eq!(verified.record().is_some(), after_commit);
        if let Some(record) = verified.record() {
            assert_eq!(record.intent().id(), intent.id());
            assert_eq!(record.grant().id(), grant.id());
            assert!(record.dispatches().is_empty());
            assert!(record.releases().is_empty());
        }
        assert_eq!(
            backend.read_state().unwrap().head,
            before,
            "read-only reconciliation retried admission"
        );
    }
}

#[derive(Default)]
struct Stop(AtomicBool);

impl CancellationSignal for Stop {
    fn request_stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct RetainedPerformer(Arc<AtomicUsize>);

impl Drop for RetainedPerformer {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl LifecyclePerformer for RetainedPerformer {
    fn identity(&self) -> Datum {
        Datum::String("performer/retained-custody".into())
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("restoration failure must not dispatch")
    }
}

struct RetainedObserver(Arc<AtomicUsize>);

impl Drop for RetainedObserver {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl PostconditionObserver for RetainedObserver {
    fn identity(&self) -> Datum {
        Datum::String("observer/retained-custody".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        panic!("restoration failure must not observe through execution")
    }
}

#[test]
fn cancellation_restore_failure_retains_every_owner_and_same_head() {
    for after_commit in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let lifecycle = OperationLifecycle::from_shared(backend.clone());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let accepted = lifecycle
            .accept_retaining(&intent, &grant, window("holder/a"))
            .unwrap();
        accepted
            .cancellation_handle(Arc::new(Stop::default()))
            .unwrap()
            .request(Datum::String("fixture/stop-before-reopen".into()))
            .unwrap();

        let reopened = lifecycle
            .accept_retaining(&intent, &grant, window("holder/b"))
            .unwrap();
        let before = backend.read_state().unwrap().head;
        let drops = Arc::new(AtomicUsize::new(0));
        let signal = Arc::new(Stop::default());
        backend.crash(1, after_commit);
        let failure = match reopened.retain_custody(
            RetainedPerformer(drops.clone()),
            RetainedObserver(drops.clone()),
            signal.clone(),
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("cancellation restoration cut unexpectedly succeeded"),
        };
        assert_eq!(
            failure.stage(),
            OperationCustodyStage::CancellationRestorationUncertain
        );
        assert_eq!(failure.operation_id(), intent.id());
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        backend.clear();
        let verified = failure.verify_retained_contract().unwrap();
        assert_eq!(
            verified.journal_head(),
            before.as_ref().map(|head| &head.entry)
        );
        let record = verified.record().unwrap();
        assert_eq!(record.intent().id(), intent.id());
        assert_eq!(record.grant().id(), grant.id());
        assert!(record.cancellation().is_some());
        assert!(record.dispatches().is_empty());
        assert!(record.releases().is_empty());
        assert_eq!(backend.read_state().unwrap().head, before);
        assert!(
            !signal.0.load(Ordering::SeqCst),
            "an uncertain restore acknowledgement cannot claim signal delivery"
        );

        drop(failure);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            2,
            "performer and observer stayed owned until retained failure dropped"
        );
    }
}
