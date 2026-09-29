use super::*;
use sim_lib_journal::{Journal, JournalEntry};

#[test]
fn canonical_second_dispatch_cannot_reuse_an_orphans_existing_lease() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    let mut performer = OrphanPerformer { allocations: 0 };
    let mut observer = OrphanObserver { satisfied: false };
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
    let record = prepared_lifecycle(backend.clone())
        .record(intent.id())
        .unwrap()
        .unwrap();
    let attempt = OperationAttempt::new(intent.id().clone(), 1).unwrap();
    let dispatch = FencedDispatch::new(
        intent.id().clone(),
        grant.id().clone(),
        attempt.id().clone(),
        record.leases()[0].id().clone(),
        performer.identity(),
    )
    .unwrap();
    let journal = Journal::new(backend.clone());
    let writer = journal.acquire_lease().unwrap();
    let head = journal.head().unwrap().unwrap();
    let objects = vec![
        JournalObject::from_datum(dispatch.canonical_datum()).unwrap(),
        JournalObject::from_datum(attempt.canonical_datum()).unwrap(),
    ];
    let entry = JournalEntry::new(
        head.sequence + 1,
        Some(head.entry.clone()),
        Symbol::qualified("operation", "lifecycle-dispatch-persisted"),
        objects.iter().map(|value| value.id.clone()).collect(),
    );
    journal
        .publish(&writer, Some(&head), objects, vec![entry])
        .unwrap();
    assert!(matches!(
        prepared_lifecycle(backend).record(intent.id()),
        Err(OperationError::InvalidTransition(
            "dispatch requires a fresh unused attempt lease"
        ))
    ));
}

struct OrphanPerformer {
    allocations: usize,
}
impl LifecyclePerformer for OrphanPerformer {
    fn identity(&self) -> Datum {
        Datum::String("fixture/orphan-owner".into())
    }
    fn plan_reservation(&self, _: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
        Ok(Some(Datum::String(
            "fixture/exact-orphan-destination".into(),
        )))
    }
    fn prepare_reserved(
        &mut self,
        _: &FencedDispatch,
        _: &crate::LifecycleReservation,
    ) -> Result<Datum, OperationError> {
        self.allocations += 1;
        Err(OperationError::PreparationUnavailable(Datum::String(
            "fixture/allocation acknowledged only by the resource owner".into(),
        )))
    }
    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("orphan reservation cannot execute")
    }
}
struct OrphanObserver {
    satisfied: bool,
}
impl PostconditionObserver for OrphanObserver {
    fn identity(&self) -> Datum {
        Datum::String("fixture/independent-orphan-observer".into())
    }
    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse {
        if self.satisfied && request.reservation().is_some() {
            PostconditionResponse::Satisfied {
                observed: request.expected().clone(),
                evidence: Datum::Nil,
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::Nil,
                evidence: Datum::Nil,
            }
        }
    }
}

#[test]
fn orphan_success_or_expired_idempotent_retry_requires_resource_reconciliation() {
    for satisfied in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::Idempotent);
        let mut performer = OrphanPerformer { allocations: 0 };
        let mut observer = OrphanObserver { satisfied };
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
        let record = prepared_lifecycle(backend.clone())
            .record(intent.id())
            .unwrap()
            .unwrap();
        assert_eq!(
            record.last_step(),
            OperationStep::ReservationIntentPersisted
        );
        assert_eq!(record.reservations().len(), 1);
        assert!(record.preparations().is_empty());
        assert!(record.releases().is_empty());
        let outcome = prepared_lifecycle(backend.clone())
            .run(
                &intent,
                &grant,
                clock_window("holder/b", 30, 40),
                &mut performer,
                &mut observer,
            )
            .unwrap();
        assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));
        assert_eq!(
            performer.allocations, 1,
            "expired lease cannot replace an orphan destination"
        );
        let record = prepared_lifecycle(backend)
            .record(intent.id())
            .unwrap()
            .unwrap();
        assert_eq!(record.dispatches().len(), 1);
        assert_eq!(record.leases().len(), 1);
        assert_eq!(record.reservations().len(), 1);
        if satisfied {
            assert!(matches!(
                record.observations().last().unwrap().response(),
                PostconditionResponse::Disputed { .. }
            ));
        }
    }
}

#[test]
fn canonical_orphan_success_and_replacement_lease_fail_strict_replay() {
    for replacement_lease in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::Idempotent);
        let mut performer = OrphanPerformer { allocations: 0 };
        let mut observer = OrphanObserver { satisfied: false };
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
        let record = prepared_lifecycle(backend.clone())
            .record(intent.id())
            .unwrap()
            .unwrap();
        let journal = Journal::new(backend.clone());
        let writer = journal.acquire_lease().unwrap();
        let (kind, datum) = if replacement_lease {
            let lease = OperationLease::new(
                intent.id().clone(),
                Datum::String("holder/b".into()),
                writer.fence(),
                30,
                40,
            )
            .unwrap()
            .in_clock(Some(Datum::String("fixture/shared-ticks".into())))
            .unwrap();
            ("lifecycle-lease-acquired", lease.canonical_datum())
        } else {
            let request = PostconditionRequest {
                operation: intent.id().clone(),
                target: intent.target().clone(),
                expected: intent.intended_result().clone(),
                dispatch: Some(record.dispatches()[0].id().clone()),
                reservation: Some(record.reservations()[0].clone()),
                preparation: None,
                release: None,
                receipt: None,
                clock: Some(Datum::String("fixture/shared-ticks".into())),
                observed_at: 30,
                last_durable_step: OperationStep::ReservationIntentPersisted,
            };
            let observation = OperationObservation::new(
                &request,
                observer.identity(),
                PostconditionResponse::Satisfied {
                    observed: request.expected().clone(),
                    evidence: Datum::Nil,
                },
            )
            .unwrap();
            let datum = observation.canonical_datum();
            assert_eq!(
                OperationObservation::from_datum(&datum).unwrap(),
                observation
            );
            ("lifecycle-observation-persisted", datum)
        };
        let head = journal.head().unwrap().unwrap();
        let object = JournalObject::from_datum(datum).unwrap();
        let entry = JournalEntry::new(
            head.sequence + 1,
            Some(head.entry.clone()),
            Symbol::qualified("operation", kind),
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
            "mutant replacement_lease={replacement_lease}"
        );
    }
}
