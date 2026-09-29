use super::*;
mod acknowledgement;
mod cancellation;
mod evidence;
mod reservation;
mod terminal;

thread_local! { static CLOCK_TICK: std::cell::Cell<u64> = const { std::cell::Cell::new(10) }; }
struct TestClock;
impl LeaseClock for TestClock {
    fn read(&self) -> Result<LeaseClockReading, OperationError> {
        Ok(LeaseClockReading {
            domain: Datum::String("fixture/shared-ticks".into()),
            tick: CLOCK_TICK.get(),
        })
    }
}
fn prepared_lifecycle(backend: Arc<CrashBackend>) -> OperationLifecycle<CrashBackend> {
    OperationLifecycle::from_shared(backend).with_clock(Arc::new(TestClock))
}
fn clock_window(holder: &str, acquired: u64, expires: u64) -> LeaseWindow {
    CLOCK_TICK.set(acquired);
    LeaseWindow::new(Datum::String(holder.into()), acquired, expires)
        .unwrap()
        .in_clock(Datum::String("fixture/shared-ticks".into()))
        .unwrap()
}

struct PreparedPerformer {
    backend: Arc<CrashBackend>,
    preparations: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
    effect: Arc<AtomicBool>,
    supersede: bool,
    supersede_after_preparation: bool,
    expire_after_preparation: bool,
    bypass_admission: bool,
}

impl LifecyclePerformer for PreparedPerformer {
    fn identity(&self) -> Datum {
        Datum::String("fixture/prepared-performer".into())
    }
    fn prepare(&mut self, _: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
        self.preparations.fetch_add(1, Ordering::SeqCst);
        if self.supersede {
            self.backend.acquire_lease()?;
        }
        Ok(Some(Datum::String(
            "fixture/exact-reserved-service-and-workspace".into(),
        )))
    }
    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("prepared performer must not take the direct path")
    }
    fn perform_prepared(
        &mut self,
        dispatch: &FencedDispatch,
        preparation: &LifecyclePreparation,
        admission: &mut dyn PreparedReleaseAdmission,
    ) -> Result<LifecyclePerformerResponse, OperationError> {
        let record = OperationLifecycle::from_shared(self.backend.clone())
            .record(dispatch.operation())
            .unwrap()
            .unwrap();
        assert_eq!(
            record.preparations().last(),
            Some(preparation),
            "preparation absent before release"
        );
        assert_eq!(preparation.dispatch(), dispatch.id());
        assert!(record.receipts().is_empty());
        if self.supersede_after_preparation {
            self.backend.acquire_lease()?;
        }
        if self.expire_after_preparation {
            CLOCK_TICK.set(20);
        }
        if self.bypass_admission {
            self.effect.store(true, Ordering::SeqCst);
            return Ok(LifecyclePerformerResponse::Receipt(Datum::String(
                "unadmitted acknowledgement".into(),
            )));
        }
        admission.admit(&mut |release: &LifecycleRelease| {
            assert_eq!(
                release.dispatch(),
                dispatch.id(),
                "release names a different dispatch"
            );
            assert_eq!(release.preparation(), preparation.id());
            assert_eq!(release.lease(), dispatch.lease());
            self.releases.fetch_add(1, Ordering::SeqCst);
            self.effect.store(true, Ordering::SeqCst);
        })?;
        CLOCK_TICK.set(CLOCK_TICK.get().max(25));
        Ok(LifecyclePerformerResponse::Receipt(Datum::String(
            "fixture/payload-completed".into(),
        )))
    }
}

struct PreparedObserver {
    effect: Arc<AtomicBool>,
}
impl PostconditionObserver for PreparedObserver {
    fn identity(&self) -> Datum {
        Datum::String("fixture/prepared-observer".into())
    }
    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse {
        if let Some(preparation) = request.preparation() {
            assert_eq!(Some(preparation.dispatch()), request.dispatch());
            assert_eq!(
                preparation.binding(),
                &Datum::String("fixture/exact-reserved-service-and-workspace".into())
            );
        }
        if self.effect.load(Ordering::SeqCst) {
            let preparation = request
                .preparation()
                .expect("completed effect needs binding");
            PostconditionResponse::Satisfied {
                observed: request.expected().clone(),
                evidence: Datum::String(preparation.id().to_string()),
            }
        } else if request.dispatch().is_some() {
            PostconditionResponse::Unavailable {
                reason: Datum::String("fixture/reserved-or-unobserved".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::Nil,
                evidence: Datum::Nil,
            }
        }
    }
}

fn prepared_ports(backend: Arc<CrashBackend>) -> (PreparedPerformer, PreparedObserver) {
    let effect = Arc::new(AtomicBool::new(false));
    (
        PreparedPerformer {
            backend,
            preparations: Arc::new(AtomicUsize::new(0)),
            releases: Arc::new(AtomicUsize::new(0)),
            effect: effect.clone(),
            supersede: false,
            supersede_after_preparation: false,
            expire_after_preparation: false,
            bypass_admission: false,
        },
        PreparedObserver { effect },
    )
}

#[test]
fn preparation_is_durable_before_payload_release() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    let result = prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/a", 10, 20),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(result, OperationOutcome::Verified { .. }));
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(performer.preparations.load(Ordering::SeqCst), 1);
    assert_eq!(performer.releases.load(Ordering::SeqCst), 1);
    let preparation = &record.preparations()[0];
    assert_eq!(
        record.observations().last().unwrap().preparation(),
        Some(preparation.id())
    );
    assert_eq!(
        LifecyclePreparation::from_datum(&preparation.canonical_datum()).unwrap(),
        *preparation
    );
    let observation = record.observations().last().unwrap();
    let release = record.releases().last().unwrap();
    assert_eq!(observation.release(), Some(release.id()));
    assert_eq!(
        record.receipts().last().unwrap().release(),
        Some(release.id())
    );
    assert_eq!(observation.observed_at(), 25);
    assert_eq!(
        observation.clock(),
        Some(&Datum::String("fixture/shared-ticks".into()))
    );
    assert_eq!(
        OperationObservation::from_datum(&observation.canonical_datum()).unwrap(),
        *observation
    );
    let mut tampered = observation.canonical_datum();
    let Datum::Node { fields, .. } = &mut tampered else {
        unreachable!()
    };
    let inner = &mut fields
        .iter_mut()
        .find(|(key, _)| key.as_qualified_str() == "observation")
        .unwrap()
        .1;
    let Datum::Node { fields, .. } = inner else {
        unreachable!()
    };
    let binding = fields
        .iter_mut()
        .find(|(key, _)| key.as_qualified_str() == "preparation")
        .unwrap();
    binding.1 = crate::operation_wire::id_datum(intent.id().content_id());
    assert!(
        OperationObservation::from_datum(&tampered).is_err(),
        "binding affects evidence identity"
    );
}

#[test]
fn a_prepared_receipt_and_satisfied_observer_cannot_bypass_release_admission() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    performer.bypass_admission = true;
    let outcome = prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/a", 10, 20),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));
    let record = prepared_lifecycle(backend.clone())
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert!(record.releases().is_empty());
    assert_eq!(
        record.receipts().len(),
        1,
        "diagnostic acknowledgement is retained"
    );
    assert!(record.receipts()[0].release().is_none());
    assert!(matches!(
        record.observations().last().unwrap().response(),
        PostconditionResponse::Disputed { .. }
    ));
    let outcome = prepared_lifecycle(backend)
        .run(
            &intent,
            &grant,
            clock_window("holder/b", 30, 40),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));
    assert_eq!(performer.preparations.load(Ordering::SeqCst), 1);
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn expiry_after_preparation_commits_intent_but_never_releases_payload() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    performer.expire_after_preparation = true;
    let result = prepared_lifecycle(backend.clone()).run(
        &intent,
        &grant,
        clock_window("holder/a", 10, 20),
        &mut performer,
        &mut observer,
    );
    assert!(
        matches!(result, Err(OperationError::InvalidLease)),
        "expired release admitted: {result:?}"
    );
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
    let record = prepared_lifecycle(backend.clone())
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.releases().len(), 1);
    assert!(record.receipts().is_empty());
    let recovered = prepared_lifecycle(backend)
        .run(
            &intent,
            &grant,
            clock_window("holder/b", 30, 40),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(recovered, OperationOutcome::Uncertain { .. }));
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
    assert_eq!(performer.preparations.load(Ordering::SeqCst), 1);
}

#[test]
fn supersession_after_preparation_prevents_release_intent_and_action() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    performer.supersede_after_preparation = true;
    assert!(
        prepared_lifecycle(backend.clone())
            .run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut performer,
                &mut observer
            )
            .is_err()
    );
    let record = prepared_lifecycle(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.preparations().len(), 1);
    assert!(record.releases().is_empty());
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn another_clock_domain_cannot_reinterpret_retained_lease_ticks() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    performer.expire_after_preparation = true;
    let _ = prepared_lifecycle(backend.clone()).run(
        &intent,
        &grant,
        clock_window("holder/a", 10, 20),
        &mut performer,
        &mut observer,
    );
    struct OtherClock;
    impl LeaseClock for OtherClock {
        fn read(&self) -> Result<LeaseClockReading, OperationError> {
            Ok(LeaseClockReading {
                domain: Datum::String("fixture/different-epoch".into()),
                tick: 30,
            })
        }
    }
    let window = LeaseWindow::new(Datum::String("holder/b".into()), 30, 40)
        .unwrap()
        .in_clock(Datum::String("fixture/different-epoch".into()))
        .unwrap();
    let result = OperationLifecycle::from_shared(backend)
        .with_clock(Arc::new(OtherClock))
        .run(&intent, &grant, window, &mut performer, &mut observer);
    assert!(matches!(result, Err(OperationError::InvalidLease)));
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn preparation_crash_cuts_never_release_before_commit_or_repeat_unknown_work() {
    for admission in 1..=9 {
        for after_commit in [false, true] {
            let backend = Arc::new(CrashBackend::default());
            backend.crash(admission, after_commit);
            let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
            let (mut performer, mut observer) = prepared_ports(backend.clone());
            let result = prepared_lifecycle(backend.clone()).run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut performer,
                &mut observer,
            );
            if admission == 5 {
                acknowledgement::assert_default_disposition(
                    result.as_ref().unwrap_err(),
                    after_commit,
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(OperationError::Journal(JournalError::InjectedCrash(_)))
                    ),
                    "{admission}/{after_commit}: {result:?}"
                );
            }
            if admission <= 5 {
                assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
            }
            backend.clear();
            let recovered = prepared_lifecycle(backend.clone())
                .run(
                    &intent,
                    &grant,
                    clock_window("holder/b", 30, 40),
                    &mut performer,
                    &mut observer,
                )
                .unwrap();
            assert!(matches!(
                recovered,
                OperationOutcome::Verified { .. } | OperationOutcome::Uncertain { .. }
            ));
            assert!(performer.releases.load(Ordering::SeqCst) <= 1);
            assert!(performer.preparations.load(Ordering::SeqCst) <= 1);
            let record = OperationLifecycle::from_shared(backend)
                .record(intent.id())
                .unwrap()
                .unwrap();
            assert!(record.dispatches().len() <= 1);
            if admission == 5 {
                assert_eq!(record.preparations().len(), usize::from(after_commit));
                assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
            }
        }
    }
}

#[test]
fn superseded_writer_cannot_release_prepared_payload() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    performer.supersede = true;
    assert!(
        prepared_lifecycle(backend.clone())
            .run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut performer,
                &mut observer
            )
            .is_err()
    );
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.dispatches().len(), 1);
    assert!(record.preparations().is_empty());
}

#[test]
fn unimplemented_prepared_release_never_falls_back_to_direct_execution() {
    struct NoRelease;
    impl LifecyclePerformer for NoRelease {
        fn identity(&self) -> Datum {
            Datum::String("fixture/no-release".into())
        }
        fn prepare(&mut self, _: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
            Ok(Some(Datum::String(
                "fixture/exact-reserved-service-and-workspace".into(),
            )))
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            panic!("unsafe fallback")
        }
    }
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (_, mut observer) = prepared_ports(backend.clone());
    let result = prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/a", 10, 20),
            &mut NoRelease,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(result, OperationOutcome::Uncertain { .. }));
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.preparations().len(), 1);
    assert!(record.receipts().is_empty());
}

#[test]
fn duplicate_preparation_is_rejected_by_durable_projection() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    let lifecycle = &mut prepared_lifecycle(backend.clone());
    lifecycle
        .run(
            &intent,
            &grant,
            clock_window("holder/a", 10, 20),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    let preparation = lifecycle
        .record(intent.id())
        .unwrap()
        .unwrap()
        .preparations()[0]
        .clone();
    let lease = backend.acquire_lease().unwrap();
    let state = backend.read_state().unwrap();
    let object = JournalObject::from_datum(preparation.canonical_datum()).unwrap();
    let head = state.head.unwrap();
    let entry = sim_lib_journal::JournalEntry::new(
        head.sequence + 1,
        Some(head.entry.clone()),
        Symbol::qualified("operation", "lifecycle-preparation-persisted"),
        vec![object.id.clone()],
    );
    Journal::new(backend.clone())
        .publish(&lease, Some(&head), vec![object], vec![entry])
        .unwrap();
    assert!(matches!(
        lifecycle.record(intent.id()),
        Err(OperationError::InvalidTransition(_))
    ));
}
