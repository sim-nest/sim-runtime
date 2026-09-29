//! Terminal sealing and outcome-observation binding regressions.

use super::*;

fn complete_prepared_snapshot() -> (
    sim_lib_journal::VerifiedSnapshot,
    OperationIntent,
    OperationGrant,
) {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer) = prepared_ports(backend.clone());
    prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/terminal", 10, 20),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    (
        Journal::new(backend).verified_snapshot().unwrap(),
        intent,
        grant,
    )
}

fn existing_payloads(snapshot: &sim_lib_journal::VerifiedSnapshot, kind: &str) -> Vec<Datum> {
    snapshot
        .entries()
        .iter()
        .find(|entry| entry.kind == Symbol::qualified("operation", kind))
        .unwrap_or_else(|| panic!("fixture event {kind} absent"))
        .payloads
        .iter()
        .map(|id| snapshot.datum(id).unwrap().clone())
        .collect()
}

fn extension(
    snapshot: &sim_lib_journal::VerifiedSnapshot,
    kind: &str,
    datums: &[Datum],
) -> (JournalEntry, Vec<JournalObject>) {
    let all_objects = datums
        .iter()
        .cloned()
        .map(JournalObject::from_datum)
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    let objects = all_objects
        .iter()
        .filter(|object| !snapshot.datums().contains_key(&object.id))
        .cloned()
        .collect();
    let head = snapshot.head().unwrap();
    (
        JournalEntry::new(
            head.sequence + 1,
            Some(head.entry.clone()),
            Symbol::qualified("operation", kind),
            all_objects.iter().map(|object| object.id.clone()).collect(),
        ),
        objects,
    )
}

fn assert_post_terminal_refused(
    snapshot: &sim_lib_journal::VerifiedSnapshot,
    kind: &str,
    datums: &[Datum],
) {
    let (entry, objects) = extension(snapshot, kind, datums);
    assert!(matches!(
        project_verified_lifecycle_extension(snapshot, &entry, &objects),
        Err(OperationError::InvalidTransition(
            "lifecycle transition after outcome"
        ))
    ));

    // Raw journal storage is deliberately policy-neutral. Reconstruct the
    // exact extended chain and prove the public closed-snapshot projector also
    // rejects it rather than relying only on writer call order.
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().unwrap();
    let original_objects = snapshot
        .datums()
        .values()
        .cloned()
        .map(JournalObject::from_datum)
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    journal
        .publish(&lease, None, original_objects, snapshot.entries().to_vec())
        .unwrap();
    journal
        .publish(&lease, snapshot.head(), objects, vec![entry])
        .unwrap();
    assert!(matches!(
        project_verified_lifecycle_records(&journal.verified_snapshot().unwrap()),
        Err(OperationError::InvalidTransition(
            "lifecycle transition after outcome"
        ))
    ));
}

#[test]
fn every_same_operation_transition_after_outcome_is_rejected() {
    let (snapshot, intent, grant) = complete_prepared_snapshot();
    let record = project_verified_lifecycle_records(&snapshot)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    let cancellation = LifecycleCancellation::new(
        intent.id().clone(),
        grant.id().clone(),
        Datum::String("too late".into()),
    )
    .unwrap();
    let reservation = LifecycleReservation::new(
        record.dispatches()[0].id().clone(),
        Datum::String("late/destination".into()),
    )
    .unwrap();

    let cases = [
        (
            "lifecycle-cancellation-intent-persisted",
            vec![cancellation.canonical_datum()],
        ),
        (
            "lifecycle-lease-acquired",
            existing_payloads(&snapshot, "lifecycle-lease-acquired"),
        ),
        (
            "lifecycle-dispatch-persisted",
            existing_payloads(&snapshot, "lifecycle-dispatch-persisted"),
        ),
        (
            "lifecycle-reservation-intent-persisted",
            vec![reservation.canonical_datum()],
        ),
        (
            "lifecycle-preparation-persisted",
            existing_payloads(&snapshot, "lifecycle-preparation-persisted"),
        ),
        (
            "lifecycle-release-intent-persisted",
            existing_payloads(&snapshot, "lifecycle-release-intent-persisted"),
        ),
        (
            "lifecycle-receipt-persisted",
            existing_payloads(&snapshot, "lifecycle-receipt-persisted"),
        ),
        (
            "lifecycle-observation-persisted",
            existing_payloads(&snapshot, "lifecycle-observation-persisted"),
        ),
        (
            "lifecycle-outcome-persisted",
            existing_payloads(&snapshot, "lifecycle-outcome-persisted"),
        ),
    ];
    for (kind, datums) in cases {
        assert_post_terminal_refused(&snapshot, kind, &datums);
    }
}

#[test]
fn later_current_observer_cannot_launder_an_older_positive_outcome() {
    let (snapshot, intent, _) = complete_prepared_snapshot();
    let record = project_verified_lifecycle_records(&snapshot)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    let prior = record.observations().last().unwrap();
    let request = PostconditionRequest {
        operation: intent.id().clone(),
        target: intent.target().clone(),
        expected: intent.intended_result().clone(),
        dispatch: prior.dispatch().cloned(),
        receipt: prior.receipt().cloned(),
        preparation: record.preparations().last().cloned(),
        reservation: record.reservations().last().cloned(),
        release: record.releases().last().cloned(),
        clock: prior.clock().cloned(),
        last_durable_step: OperationStep::OutcomePersisted,
        observed_at: prior.observed_at() + 1,
    };
    let later = OperationObservation::new(
        &request,
        Datum::String("fixture/current-observer-b".into()),
        PostconditionResponse::Satisfied {
            observed: intent.intended_result().clone(),
            evidence: Datum::String("later observer cannot rebind outcome".into()),
        },
    )
    .unwrap();
    assert_post_terminal_refused(
        &snapshot,
        "lifecycle-observation-persisted",
        &[later.canonical_datum()],
    );
}

#[test]
fn outcome_must_name_the_exact_observation_it_consumes() {
    let (snapshot, intent, _) = complete_prepared_snapshot();
    let cut = snapshot.entries().len() - 1;
    let prefix_entries = snapshot.entries()[..cut].to_vec();
    let ids = prefix_entries
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let objects = ids
        .into_iter()
        .map(|id| JournalObject::from_datum(snapshot.datum(id).unwrap().clone()).unwrap())
        .collect();
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().unwrap();
    journal
        .publish(&lease, None, objects, prefix_entries)
        .unwrap();
    let prefix = journal.verified_snapshot().unwrap();
    let record = project_verified_lifecycle_records(&prefix)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    let wrong = crate::lifecycle_record::IdentifiedOutcome::new(
        intent.id().clone(),
        record.observations()[0].id().clone(),
        OperationOutcome::Verified {
            evidence: record.observations().last().unwrap().evidence().clone(),
        },
    )
    .unwrap();
    let (entry, objects) = extension(
        &prefix,
        "lifecycle-outcome-persisted",
        &[wrong.canonical_datum()],
    );
    assert!(matches!(
        project_verified_lifecycle_extension(&prefix, &entry, &objects),
        Err(OperationError::InvalidTransition(
            "outcome observation mismatch"
        ))
    ));
}

#[test]
fn a_negative_postcondition_after_release_is_uncertain_not_diverged() {
    // A negative postcondition is not a resource-disposition proof (the same
    // rule the recovery path already applies to an existing record): a
    // preparation or reservation this dispatch created may not have reached
    // release admission by the time it is independently observed, so a
    // terminal Diverged here would strand custody rather than let
    // reconciliation resolve it. This exercises the fresh-dispatch path
    // (not resume), immediately after perform_prepared, not a later replay.
    struct AlwaysNotSatisfied;
    impl PostconditionObserver for AlwaysNotSatisfied {
        fn identity(&self) -> Datum {
            Datum::String("fixture/always-not-satisfied-observer".into())
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
    let (mut performer, _) = prepared_ports(backend.clone());
    let outcome = prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/a", 10, 20),
            &mut performer,
            &mut AlwaysNotSatisfied,
        )
        .unwrap();
    assert!(
        matches!(outcome, OperationOutcome::Uncertain { .. }),
        "a completed release must not be sealed Diverged on a negative postcondition: {outcome:?}"
    );
    assert_eq!(
        performer.releases.load(Ordering::SeqCst),
        1,
        "the release genuinely admitted, proving this is not simply the no-custody case"
    );
    let record = prepared_lifecycle(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert!(
        matches!(record.outcome(), Some(OperationOutcome::Uncertain { .. })),
        "the Uncertain outcome itself must be durably persisted, not just returned: {:?}",
        record.outcome()
    );
}

#[test]
fn projector_refuses_a_diverged_outcome_over_unresolved_custody() {
    // The engine itself now never produces Diverged when a reservation,
    // preparation, or cancellation is unresolved (see
    // a_negative_postcondition_after_release_is_uncertain_not_diverged
    // above). This proves the SAME rule independently at the projection
    // layer: raw journal storage is policy-neutral, so a canonical
    // extension claiming Diverged over the identical durable facts must be
    // rejected on its own, not merely because the engine happens not to
    // write one.
    struct AlwaysNotSatisfied;
    impl PostconditionObserver for AlwaysNotSatisfied {
        fn identity(&self) -> Datum {
            Datum::String("fixture/always-not-satisfied-observer".into())
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
    let (mut performer, _) = prepared_ports(backend.clone());
    let outcome = prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/b", 10, 20),
            &mut performer,
            &mut AlwaysNotSatisfied,
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));
    assert_eq!(performer.releases.load(Ordering::SeqCst), 1);
    let snapshot = Journal::new(backend).verified_snapshot().unwrap();
    // The engine wrote Uncertain as its last entry; cut it to reach the
    // same prefix state (release admitted, observation NotSatisfied, no
    // outcome yet) that a hostile or buggy writer would also start from.
    let cut = snapshot.entries().len() - 1;
    let prefix_entries = snapshot.entries()[..cut].to_vec();
    let ids = prefix_entries
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let objects = ids
        .into_iter()
        .map(|id| JournalObject::from_datum(snapshot.datum(id).unwrap().clone()).unwrap())
        .collect();
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().unwrap();
    journal
        .publish(&lease, None, objects, prefix_entries)
        .unwrap();
    let prefix = journal.verified_snapshot().unwrap();
    let record = project_verified_lifecycle_records(&prefix)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    assert!(
        !record.reservations().is_empty() || !record.preparations().is_empty(),
        "fixture must retain genuine unresolved custody, or this test proves nothing"
    );
    let forged = crate::lifecycle_record::IdentifiedOutcome::new(
        intent.id().clone(),
        record.observations().last().unwrap().id().clone(),
        OperationOutcome::Diverged {
            observed: Datum::Nil,
            expected: intent.intended_result().clone(),
        },
    )
    .unwrap();
    let (entry, objects) = extension(
        &prefix,
        "lifecycle-outcome-persisted",
        &[forged.canonical_datum()],
    );
    assert!(matches!(
        project_verified_lifecycle_extension(&prefix, &entry, &objects),
        Err(OperationError::InvalidTransition(_))
    ));
}

#[test]
fn a_late_receipt_after_the_observation_leaves_it_stale_for_any_outcome() {
    // A receipt is not required to arrive before its dispatch's own
    // observation -- AcknowledgementMissing observed first, a late receipt
    // second, is a real sequence the projector's own per-entry admission
    // rules already permit (neither checks last_step). That means a
    // canonical outcome still citing the pre-receipt observation is stale:
    // it was taken before the receipt existed, so it cannot speak to
    // whatever the receipt's own content now durably records. Naming the
    // record's current last dispatch (the older, narrower check) does not
    // catch this, because the receipt never changes which dispatch is
    // last -- only the record's generation-based freshness check does.
    struct NoAck;
    impl LifecyclePerformer for NoAck {
        fn identity(&self) -> Datum {
            Datum::String("fixture/no-ack".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
    struct AlwaysNotSatisfied;
    impl PostconditionObserver for AlwaysNotSatisfied {
        fn identity(&self) -> Datum {
            Datum::String("fixture/always-not-satisfied-observer".into())
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            PostconditionResponse::NotSatisfied {
                observed: Datum::Nil,
                evidence: Datum::Nil,
            }
        }
    }

    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/late-receipt", 10, 20),
            &mut NoAck,
            &mut AlwaysNotSatisfied,
        )
        .unwrap();
    let snapshot = Journal::new(backend).verified_snapshot().unwrap();

    // Cut the engine's own sealed outcome, reaching the same prefix (a
    // dispatch, no receipt, one NotSatisfied observation, no outcome yet)
    // a hostile or buggy writer would also start from.
    let cut = snapshot.entries().len() - 1;
    let prefix_entries = snapshot.entries()[..cut].to_vec();
    let prefix_ids = prefix_entries
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let prefix_objects: Vec<_> = prefix_ids
        .into_iter()
        .map(|id| JournalObject::from_datum(snapshot.datum(id).unwrap().clone()).unwrap())
        .collect();
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().unwrap();
    journal
        .publish(&lease, None, prefix_objects, prefix_entries)
        .unwrap();
    let prefix = journal.verified_snapshot().unwrap();
    let record = project_verified_lifecycle_records(&prefix)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    assert!(
        record.outcome().is_none(),
        "fixture must end before an outcome, or this test proves nothing"
    );
    let stale_observation = record.observations().last().unwrap().clone();
    let dispatch_id = record.dispatches().last().unwrap().id().clone();

    // Forge the late receipt and admit it for real -- the projector's own
    // rules permit it, matching the scenario's own premise.
    let receipt =
        crate::lifecycle::LifecycleReceipt::new(dispatch_id, Datum::String("late".into()), None)
            .unwrap();
    let (receipt_entry, receipt_objects) = extension(
        &prefix,
        "lifecycle-receipt-persisted",
        &[receipt.canonical_datum()],
    );
    project_verified_lifecycle_extension(&prefix, &receipt_entry, &receipt_objects)
        .expect("a late receipt is a legal admission on its own");
    let receipt_lease = journal.acquire_lease().unwrap();
    journal
        .publish(
            &receipt_lease,
            prefix.head().cloned().as_ref(),
            receipt_objects,
            vec![receipt_entry],
        )
        .unwrap();
    let with_receipt = journal.verified_snapshot().unwrap();

    // Now forge an outcome citing the observation that predates the
    // receipt -- still the record's ONLY observation, still naming its
    // current last dispatch, but no longer the record's freshest fact.
    let forged = crate::lifecycle_record::IdentifiedOutcome::new(
        intent.id().clone(),
        stale_observation.id().clone(),
        OperationOutcome::Uncertain {
            last_durable_step: OperationStep::ReceiptPersisted,
        },
    )
    .unwrap();
    let (outcome_entry, outcome_objects) = extension(
        &with_receipt,
        "lifecycle-outcome-persisted",
        &[forged.canonical_datum()],
    );
    assert!(matches!(
        project_verified_lifecycle_extension(&with_receipt, &outcome_entry, &outcome_objects),
        Err(OperationError::InvalidTransition(_))
    ));
}

#[test]
fn re_appending_the_records_own_existing_observation_is_refused_as_a_replay() {
    // Without this check, a canonical writer could re-emit the record's
    // own already-recorded observation as a "new" journal entry -- the
    // observation-persisted branch's own admission checks (dispatch
    // match, receipt match, etc.) all still pass, since it is byte-
    // identical to the one already on file. That would re-stamp
    // generation/observation_generation to the new entry's sequence,
    // making genuinely stale evidence (recorded before a later lease or
    // receipt) look fresh again to the generation-based staleness check.
    struct NoAck;
    impl LifecyclePerformer for NoAck {
        fn identity(&self) -> Datum {
            Datum::String("fixture/no-ack".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
    struct AlwaysNotSatisfied;
    impl PostconditionObserver for AlwaysNotSatisfied {
        fn identity(&self) -> Datum {
            Datum::String("fixture/always-not-satisfied-observer".into())
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            PostconditionResponse::NotSatisfied {
                observed: Datum::Nil,
                evidence: Datum::Nil,
            }
        }
    }

    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::Idempotent);
    prepared_lifecycle(backend.clone())
        .run(
            &intent,
            &grant,
            clock_window("holder/replay", 10, 20),
            &mut NoAck,
            &mut AlwaysNotSatisfied,
        )
        .unwrap();
    let snapshot = Journal::new(backend).verified_snapshot().unwrap();
    let cut = snapshot.entries().len() - 1;
    let prefix_entries = snapshot.entries()[..cut].to_vec();
    let prefix_ids = prefix_entries
        .iter()
        .flat_map(|entry| entry.payloads.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let prefix_objects: Vec<_> = prefix_ids
        .into_iter()
        .map(|id| JournalObject::from_datum(snapshot.datum(id).unwrap().clone()).unwrap())
        .collect();
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal.acquire_lease().unwrap();
    journal
        .publish(&lease, None, prefix_objects, prefix_entries)
        .unwrap();
    let prefix = journal.verified_snapshot().unwrap();
    let record = project_verified_lifecycle_records(&prefix)
        .unwrap()
        .remove(intent.id())
        .unwrap();
    assert!(
        record.outcome().is_none(),
        "fixture must end before an outcome, or this test proves nothing"
    );
    let existing_observation = record.observations().last().unwrap().clone();

    // Re-append the SAME observation (byte-identical, so it passes every
    // other admission check) as a "new" entry.
    let (entry, objects) = extension(
        &prefix,
        "lifecycle-observation-persisted",
        &[existing_observation.canonical_datum()],
    );
    assert!(matches!(
        project_verified_lifecycle_extension(&prefix, &entry, &objects),
        Err(OperationError::InvalidTransition(_))
    ));
}
