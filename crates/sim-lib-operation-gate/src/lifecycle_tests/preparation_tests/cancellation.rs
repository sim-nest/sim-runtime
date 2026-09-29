use super::*;

#[derive(Default)]
struct Stop(AtomicBool);
impl CancellationSignal for Stop {
    fn request_stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
fn reason() -> Datum {
    Datum::String("fixture/explicit-stop".into())
}

#[test]
fn cancellation_winning_after_validation_invalidates_dispatch_and_release_cas() {
    for kind in [
        "lifecycle-dispatch-persisted",
        "lifecycle-release-intent-persisted",
    ] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let lifecycle = prepared_lifecycle(backend.clone());
        let accepted = lifecycle
            .accept(&intent, &grant, clock_window("holder/a", 10, 20))
            .unwrap();
        let control = accepted
            .cancellation_handle(Arc::new(Stop::default()))
            .unwrap();
        *backend.before_admission.lock().unwrap() = Some(BeforeAdmission {
            kind: Symbol::qualified("operation", kind),
            action: Box::new(move || {
                control.request(reason()).unwrap();
            }),
        });
        let (mut performer, mut observer) = prepared_ports(backend.clone());
        assert!(matches!(
            accepted.run(&mut performer, &mut observer),
            Err(OperationError::Journal(
                JournalError::ConflictingDelivery | JournalError::WrongHead
            ))
        ));
        let record = lifecycle.record(intent.id()).unwrap().unwrap();
        assert!(record.cancellation().is_some());
        assert!(record.releases().is_empty());
        assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
        if kind == "lifecycle-dispatch-persisted" {
            assert!(record.dispatches().is_empty());
            assert_eq!(performer.preparations.load(Ordering::SeqCst), 0);
        } else {
            assert_eq!(record.preparations().len(), 1);
        }
    }
}

#[test]
fn accepted_cancellation_reopens_without_allocating_or_claiming_quiescence() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    let lifecycle = prepared_lifecycle(backend.clone());
    let accepted = lifecycle
        .accept(&intent, &grant, clock_window("holder/a", 10, 20))
        .unwrap();
    let stopped = Arc::new(Stop::default());
    let control = accepted.cancellation_handle(stopped.clone()).unwrap();
    let cancellation = control.request(reason()).unwrap();
    assert!(stopped.0.load(Ordering::SeqCst));
    assert_eq!(
        LifecycleCancellation::from_datum(&cancellation.canonical_datum()).unwrap(),
        cancellation
    );
    let before = backend.read_state().unwrap().head;
    assert_eq!(
        control
            .request(Datum::String("fixture/redelivery".into()))
            .unwrap(),
        cancellation
    );
    assert_eq!(before, backend.read_state().unwrap().head);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    assert!(matches!(
        accepted.run(&mut performer, &mut observer).unwrap(),
        OperationOutcome::Uncertain { .. }
    ));
    let reopened = lifecycle
        .accept(&intent, &grant, clock_window("holder/b", 30, 40))
        .unwrap();
    let restored = Arc::new(Stop::default());
    reopened.cancellation_handle(restored.clone()).unwrap();
    assert!(restored.0.load(Ordering::SeqCst));
    assert!(matches!(
        reopened.run(&mut performer, &mut observer).unwrap(),
        OperationOutcome::Uncertain { .. }
    ));
    let record = lifecycle.record(intent.id()).unwrap().unwrap();
    assert_eq!(record.cancellation(), Some(&cancellation));
    assert!(record.dispatches().is_empty());
    assert!(record.leases().is_empty());
    assert_eq!(performer.preparations.load(Ordering::SeqCst), 0);
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn cancellation_acknowledgement_loss_preserves_intent_and_stale_handles_cannot_signal() {
    for after_commit in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let lifecycle = prepared_lifecycle(backend.clone());
        let accepted = lifecycle
            .accept(&intent, &grant, clock_window("holder/a", 10, 20))
            .unwrap();
        let signal = Arc::new(Stop::default());
        let handle = accepted.cancellation_handle(signal.clone()).unwrap();
        backend.crash(1, after_commit);
        assert!(handle.request(reason()).is_err());
        backend.clear();
        assert_eq!(
            lifecycle
                .record(intent.id())
                .unwrap()
                .unwrap()
                .cancellation()
                .is_some(),
            after_commit
        );
        assert_eq!(signal.0.load(Ordering::SeqCst), after_commit);
        let replacement = lifecycle
            .accept(&intent, &grant, clock_window("holder/b", 30, 40))
            .unwrap();
        let restored = Arc::new(Stop::default());
        replacement.cancellation_handle(restored.clone()).unwrap();
        assert_eq!(restored.0.load(Ordering::SeqCst), after_commit);
        signal.0.store(false, Ordering::SeqCst);
        assert!(matches!(
            handle.request(reason()),
            Err(OperationError::Journal(JournalError::StaleLease))
        ));
        assert!(!signal.0.load(Ordering::SeqCst));
    }
}

struct CancelPerformer {
    inner: PreparedPerformer,
    control: CancellationHandle<CrashBackend>,
    during_preparation: bool,
    after_release: bool,
}
impl LifecyclePerformer for CancelPerformer {
    fn identity(&self) -> Datum {
        self.inner.identity()
    }
    fn prepare(&mut self, dispatch: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
        let result = self.inner.prepare(dispatch)?;
        if self.during_preparation {
            self.control.request(reason())?;
        }
        Ok(result)
    }
    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("direct release forbidden")
    }
    fn perform_prepared(
        &mut self,
        dispatch: &FencedDispatch,
        preparation: &LifecyclePreparation,
        admission: &mut dyn PreparedReleaseAdmission,
    ) -> Result<LifecyclePerformerResponse, OperationError> {
        if !self.after_release {
            self.control.request(reason())?;
        }
        let result = self
            .inner
            .perform_prepared(dispatch, preparation, admission)?;
        if self.after_release {
            self.control.request(reason())?;
        }
        Ok(result)
    }
}

#[test]
fn cancellation_is_orthogonal_to_preparation_and_serialized_against_release() {
    for (during_preparation, after_release) in [(true, false), (false, false), (false, true)] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let lifecycle = prepared_lifecycle(backend.clone());
        let accepted = lifecycle
            .accept(&intent, &grant, clock_window("holder/a", 10, 20))
            .unwrap();
        let control = accepted
            .cancellation_handle(Arc::new(Stop::default()))
            .unwrap();
        let (inner, mut observer) = prepared_ports(backend.clone());
        let mut performer = CancelPerformer {
            inner,
            control,
            during_preparation,
            after_release,
        };
        let outcome = accepted.run(&mut performer, &mut observer);
        if after_release {
            assert!(matches!(
                outcome.unwrap(),
                OperationOutcome::Verified { .. }
            ));
        } else {
            assert!(matches!(outcome, Err(OperationError::Cancelled)));
        }
        let record = lifecycle.record(intent.id()).unwrap().unwrap();
        assert_eq!(
            record.preparations().len(),
            1,
            "truthful preparation is retained after cancellation"
        );
        assert!(record.cancellation().is_some());
        assert_eq!(record.releases().len(), usize::from(after_release));
        assert_eq!(
            performer.inner.releases.load(Ordering::SeqCst),
            usize::from(after_release)
        );
        assert_eq!(record.receipts().len(), usize::from(after_release));
    }
}

struct CancellingObserver {
    inner: PreparedObserver,
    control: CancellationHandle<CrashBackend>,
}
impl PostconditionObserver for CancellingObserver {
    fn identity(&self) -> Datum {
        self.inner.identity()
    }
    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse {
        self.control.request(reason()).unwrap();
        self.inner.observe(request)
    }
}

#[test]
fn cancellation_after_run_snapshot_cannot_be_rebased_into_a_new_dispatch() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    let lifecycle = prepared_lifecycle(backend.clone());
    let accepted = lifecycle
        .accept(&intent, &grant, clock_window("holder/a", 10, 20))
        .unwrap();
    let control = accepted
        .cancellation_handle(Arc::new(Stop::default()))
        .unwrap();
    let (mut performer, inner) = prepared_ports(backend.clone());
    let mut observer = CancellingObserver { inner, control };
    assert!(matches!(
        accepted.run(&mut performer, &mut observer),
        Err(OperationError::Cancelled)
    ));
    let record = lifecycle.record(intent.id()).unwrap().unwrap();
    assert!(record.cancellation().is_some());
    assert!(record.leases().is_empty());
    assert!(record.dispatches().is_empty());
    assert_eq!(performer.preparations.load(Ordering::SeqCst), 0);
}

#[test]
fn cancellation_is_reachable_while_performer_blocks_after_release() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let lifecycle = prepared_lifecycle(backend.clone());
    let accepted = lifecycle
        .accept(&intent, &grant, clock_window("holder/a", 10, 20))
        .unwrap();
    let signal = Arc::new(Stop::default());
    let control = accepted.cancellation_handle(signal.clone()).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(1);
    struct Blocking {
        inner: PreparedPerformer,
        started: std::sync::mpsc::SyncSender<()>,
        finish: std::sync::mpsc::Receiver<()>,
    }
    impl LifecyclePerformer for Blocking {
        fn identity(&self) -> Datum {
            self.inner.identity()
        }
        fn prepare(&mut self, dispatch: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
            self.inner.prepare(dispatch)
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            panic!("direct path")
        }
        fn perform_prepared(
            &mut self,
            dispatch: &FencedDispatch,
            preparation: &LifecyclePreparation,
            admission: &mut dyn PreparedReleaseAdmission,
        ) -> Result<LifecyclePerformerResponse, OperationError> {
            let result = self
                .inner
                .perform_prepared(dispatch, preparation, admission)?;
            self.started.send(()).unwrap();
            self.finish
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(result)
        }
    }
    let (inner, mut observer) = prepared_ports(backend.clone());
    let mut performer = Blocking {
        inner,
        started: started_tx,
        finish: finish_rx,
    };
    let thread = std::thread::spawn(move || accepted.run(&mut performer, &mut observer));
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    control.request(reason()).unwrap();
    assert!(signal.0.load(Ordering::SeqCst));
    let during = lifecycle.record(intent.id()).unwrap().unwrap();
    assert!(during.cancellation().is_some());
    assert!(
        during.receipts().is_empty(),
        "cancellation is not completion"
    );
    assert!(during.outcome().is_none());
    finish_tx.send(()).unwrap();
    assert!(matches!(
        thread.join().unwrap().unwrap(),
        OperationOutcome::Verified { .. }
    ));
}

#[test]
fn direct_perform_cancelled_mid_flight_is_uncertain_not_diverged() {
    // A negative postcondition is not a resource-disposition proof: a
    // durable cancellation requested during the direct (no preparation, no
    // reservation) performer's own perform() call, landing before the
    // outcome is decided, must not seal Diverged -- the recovery path
    // already treats this exact durable fact as Uncertain for an existing
    // record; the fresh-dispatch path must match it, with no custody
    // involved at all, proving this is the cancellation check specifically.
    struct CancellingPerformer {
        control: CancellationHandle<CrashBackend>,
    }
    impl LifecyclePerformer for CancellingPerformer {
        fn identity(&self) -> Datum {
            Datum::String("fixture/cancelling-direct-performer".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            self.control.request(reason()).unwrap();
            LifecyclePerformerResponse::Receipt(Datum::String("fixture/raced-payload".into()))
        }
    }
    struct NeverSatisfied;
    impl PostconditionObserver for NeverSatisfied {
        fn identity(&self) -> Datum {
            Datum::String("fixture/never-satisfied-observer".into())
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            PostconditionResponse::NotSatisfied {
                observed: Datum::Nil,
                evidence: Datum::Nil,
            }
        }
    }
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let lifecycle = prepared_lifecycle(backend.clone());
    let accepted = lifecycle
        .accept(&intent, &grant, clock_window("holder/a", 10, 20))
        .unwrap();
    let control = accepted
        .cancellation_handle(Arc::new(Stop::default()))
        .unwrap();
    let outcome = accepted
        .run(&mut CancellingPerformer { control }, &mut NeverSatisfied)
        .unwrap();
    assert!(
        matches!(outcome, OperationOutcome::Uncertain { .. }),
        "a cancellation that raced with a direct perform must not be sealed Diverged: {outcome:?}"
    );
    let record = lifecycle.record(intent.id()).unwrap().unwrap();
    assert!(record.cancellation().is_some());
    assert!(record.reservations().is_empty());
    assert!(record.preparations().is_empty());
    assert!(
        matches!(record.outcome(), Some(OperationOutcome::Uncertain { .. })),
        "the Uncertain outcome itself must be durably persisted: {:?}",
        record.outcome()
    );
}
