use super::*;
use sim_lib_journal::{Journal, JournalEntry};

#[test]
fn clock_loss_domain_change_or_rewind_after_release_preserves_recovery() {
    struct InterruptedClock {
        releases: Arc<AtomicUsize>,
        fault: usize,
    }
    impl LeaseClock for InterruptedClock {
        fn read(&self) -> Result<LeaseClockReading, OperationError> {
            let released = self.releases.load(Ordering::SeqCst) != 0;
            if released && self.fault == 0 {
                return Err(OperationError::InvalidLease);
            }
            Ok(LeaseClockReading {
                domain: Datum::String(if released && self.fault == 1 {
                    "fixture/foreign-clock".into()
                } else {
                    "fixture/shared-ticks".into()
                }),
                tick: if released && self.fault == 2 { 9 } else { 10 },
            })
        }
    }
    for fault in 0..3 {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let (mut performer, mut observer) = prepared_ports(backend.clone());
        let result = OperationLifecycle::from_shared(backend.clone())
            .with_clock(Arc::new(InterruptedClock {
                releases: performer.releases.clone(),
                fault,
            }))
            .run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut performer,
                &mut observer,
            );
        assert!(
            matches!(result, Err(OperationError::InvalidLease)),
            "{fault}: {result:?}"
        );
        let record = prepared_lifecycle(backend.clone())
            .record(intent.id())
            .unwrap()
            .unwrap();
        assert_eq!(record.last_step(), OperationStep::ReceiptPersisted);
        assert_eq!(
            record.observations().len(),
            1,
            "no fabricated post-release observation"
        );
        assert_eq!(record.receipts().len(), 1, "diagnostic receipt is retained");
        assert_eq!(record.releases().len(), 1);
        let recovered = prepared_lifecycle(backend)
            .run(
                &intent,
                &grant,
                clock_window("holder/b", 30, 40),
                &mut performer,
                &mut observer,
            )
            .unwrap();
        assert!(matches!(recovered, OperationOutcome::Verified { .. }));
        assert_eq!(
            performer.releases.load(Ordering::SeqCst),
            1,
            "recovery must not replay release"
        );
    }
}

#[test]
fn canonical_reencoding_cannot_launder_release_or_clock_substitution() {
    for mutation in 0..5 {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let (mut performer, mut observer) = prepared_ports(backend.clone());
        backend.crash(7, true);
        assert!(
            prepared_lifecycle(backend.clone())
                .run(
                    &intent,
                    &grant,
                    clock_window("holder/a", 10, 20),
                    &mut performer,
                    &mut observer,
                )
                .is_err()
        );
        backend.clear();
        let record = prepared_lifecycle(backend.clone())
            .record(intent.id())
            .unwrap()
            .unwrap();
        assert_eq!(record.last_step(), OperationStep::ReceiptPersisted);
        let mut request = PostconditionRequest {
            reservation: None,
            operation: intent.id().clone(),
            target: intent.target().clone(),
            expected: intent.intended_result().clone(),
            dispatch: Some(record.dispatches()[0].id().clone()),
            receipt: Some(record.receipts()[0].id().clone()),
            preparation: Some(record.preparations()[0].clone()),
            release: Some(record.releases()[0].clone()),
            clock: Some(Datum::String("fixture/shared-ticks".into())),
            observed_at: 25,
            last_durable_step: OperationStep::ReceiptPersisted,
        };
        match mutation {
            0 => request.release = None,
            1 => {
                let mut release = record.releases()[0].canonical_datum();
                let Datum::Node { fields, .. } = &mut release else {
                    unreachable!()
                };
                fields
                    .iter_mut()
                    .find(|(key, _)| key.as_qualified_str() == "lease")
                    .unwrap()
                    .1 = crate::operation_wire::id_datum(intent.id().content_id());
                request.release = Some(LifecycleRelease::from_datum(&release).unwrap());
            }
            2 => request.clock = None,
            3 => request.clock = Some(Datum::String("fixture/foreign-clock".into())),
            4 => request.observed_at = 9,
            _ => unreachable!(),
        }
        let observation = OperationObservation::new(
            &request,
            observer.identity(),
            PostconditionResponse::Satisfied {
                observed: intent.intended_result().clone(),
                evidence: Datum::Nil,
            },
        )
        .unwrap();
        let datum = observation.canonical_datum();
        assert_eq!(
            OperationObservation::from_datum(&datum).unwrap(),
            observation,
            "the mutant has fully recomputed canonical identities"
        );
        let journal = Journal::new(backend.clone());
        let writer = journal.acquire_lease().unwrap();
        let head = journal.head().unwrap().unwrap();
        let object = JournalObject::from_datum(datum).unwrap();
        let entry = JournalEntry::new(
            head.sequence + 1,
            Some(head.entry.clone()),
            Symbol::qualified("operation", "lifecycle-observation-persisted"),
            vec![object.id.clone()],
        );
        journal
            .publish(&writer, Some(&head), vec![object], vec![entry])
            .unwrap();
        assert!(
            matches!(
                prepared_lifecycle(backend).record(intent.id()),
                Err(OperationError::InvalidTransition(_))
            ),
            "mutant {mutation} projected"
        );
        assert_eq!(performer.releases.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn retained_prepared_success_without_release_never_projects_or_returns_cached_success() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    backend.crash(5, true);
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
    backend.clear();
    let record = prepared_lifecycle(backend.clone())
        .record(intent.id())
        .unwrap()
        .unwrap();
    let request = PostconditionRequest {
        reservation: None,
        operation: intent.id().clone(),
        target: intent.target().clone(),
        expected: intent.intended_result().clone(),
        dispatch: Some(record.dispatches()[0].id().clone()),
        receipt: None,
        preparation: Some(record.preparations()[0].clone()),
        release: None,
        clock: None,
        observed_at: 12,
        last_durable_step: OperationStep::PreparationPersisted,
    };
    let observation = OperationObservation::new(
        &request,
        observer.identity(),
        PostconditionResponse::Satisfied {
            observed: intent.intended_result().clone(),
            evidence: Datum::String("matching-output".into()),
        },
    )
    .unwrap();
    let outcome = crate::lifecycle_record::IdentifiedOutcome::new(
        intent.id().clone(),
        observation.id().clone(),
        OperationOutcome::Verified {
            evidence: observation.evidence().clone(),
        },
    )
    .unwrap();
    let journal = Journal::new(backend.clone());
    let writer = journal.acquire_lease().unwrap();
    for (kind, datum) in [
        (
            "lifecycle-observation-persisted",
            observation.canonical_datum(),
        ),
        ("lifecycle-outcome-persisted", outcome.canonical_datum()),
    ] {
        let object = JournalObject::from_datum(datum).unwrap();
        let head = journal.head().unwrap().unwrap();
        let entry = JournalEntry::new(
            head.sequence + 1,
            Some(head.entry.clone()),
            Symbol::qualified("operation", kind),
            vec![object.id.clone()],
        );
        journal
            .publish(&writer, Some(&head), vec![object], vec![entry])
            .unwrap();
    }
    assert!(matches!(
        prepared_lifecycle(backend.clone()).record(intent.id()),
        Err(OperationError::InvalidTransition(
            "reserved or prepared success without release intent"
        ))
    ));
    assert!(matches!(
        prepared_lifecycle(backend).run(
            &intent,
            &grant,
            clock_window("holder/b", 30, 40),
            &mut performer,
            &mut observer
        ),
        Err(OperationError::InvalidTransition(
            "reserved or prepared success without release intent"
        ))
    ));
    assert_eq!(performer.releases.load(Ordering::SeqCst), 0);
}

#[test]
fn direct_perform_refuses_a_receipt_after_lease_expiry() {
    // The prepared path's admission already re-checks lease liveness
    // atomically at release-commit time (see the expiry_after_preparation_*
    // test in preparation_tests.rs). The direct (unprepared) path -- no
    // preparation, no reservation -- must apply the same rule at its own
    // commit point (the receipt), not only validate the window once at
    // accept time.
    struct ExpiringPerformer;
    impl LifecyclePerformer for ExpiringPerformer {
        fn identity(&self) -> Datum {
            Datum::String("fixture/expiring-direct-performer".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            CLOCK_TICK.set(20);
            LifecyclePerformerResponse::Receipt(Datum::String("fixture/late-payload".into()))
        }
    }
    // Not-satisfied throughout: the pre-dispatch check (before performer.perform
    // ever runs) needs a real, non-panicking response too, since every call
    // observes once before attempting dispatch.
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
    let result = prepared_lifecycle(backend.clone()).run(
        &intent,
        &grant,
        clock_window("holder/a", 10, 20),
        &mut ExpiringPerformer,
        &mut NeverSatisfied,
    );
    assert!(
        matches!(result, Err(OperationError::InvalidLease)),
        "expired direct-perform receipt admitted: {result:?}"
    );
    let record = prepared_lifecycle(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert!(
        record.outcome().is_none(),
        "no outcome may be finalized from a refused commit"
    );
}
