//! Same-snapshot verified operation projection regressions.

use std::sync::Arc;

use sim_kernel::Datum;
use sim_lib_journal::Journal;

use super::*;

#[test]
fn projection_and_head_remain_bound_across_reopen() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let empty = OperationLifecycle::from_shared(backend.clone())
        .verified_record(intent.id())
        .expect("empty verified read");
    assert_eq!(empty.journal_head(), None);
    assert_eq!(empty.journal_sequence(), None);
    assert_eq!(empty.record(), None);

    let accepted = OperationLifecycle::from_shared(backend.clone())
        .accept(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
        )
        .expect("durably accept operation");
    drop(accepted);

    let reopened = OperationLifecycle::from_shared(backend.clone())
        .verified_record(intent.id())
        .expect("reopened verified read");
    let independent = Journal::new(backend).verified_snapshot().unwrap();
    let expected_head = independent.head().expect("accepted journal head");
    assert_eq!(reopened.journal_head(), Some(&expected_head.entry));
    assert_eq!(reopened.journal_sequence(), Some(expected_head.sequence));
    let record = reopened.record().expect("accepted operation record");
    assert_eq!(record.intent().id(), intent.id());
    assert_eq!(record.last_step(), OperationStep::IntentPersisted);
    assert_eq!(record.outcome(), None);

    let rebound = OperationLifecycle::<CrashBackend>::verified_record_from_snapshot(
        &independent,
        intent.id(),
    )
    .expect("project supplied snapshot");
    assert_eq!(rebound, reopened);
}

#[test]
fn only_exact_retained_contract_produces_the_opaque_same_read_view() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
    let (mut performer, mut observer, _, _, _) = ports(true);
    OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/current".into()), 10, 20).unwrap(),
            &mut performer,
            &mut observer,
        )
        .unwrap();
    let read = OperationLifecycle::from_shared(backend)
        .verified_record(intent.id())
        .unwrap();

    let retained = read
        .validate_retained_contract(
            &Datum::String("performer/fake".into()),
            &Datum::String("observer/independent".into()),
        )
        .unwrap();
    assert_eq!(retained.journal_head(), read.journal_head());
    assert_eq!(retained.journal_sequence(), read.journal_sequence());
    assert_eq!(retained.record(), read.record());

    assert!(matches!(
        read.validate_retained_contract(
            &Datum::String("performer/replaced".into()),
            &Datum::String("observer/independent".into()),
        ),
        Err(OperationError::PerformerMismatch)
    ));
    assert!(matches!(
        read.validate_retained_contract(
            &Datum::String("performer/fake".into()),
            &Datum::String("observer/replaced".into()),
        ),
        Err(OperationError::ObserverMismatch)
    ));
}

#[test]
fn absent_read_still_requires_distinct_canonical_contract_identities() {
    let backend = Arc::new(CrashBackend::default());
    let (intent, _) = fixture(ReplayPolicy::ExactlyOnce);
    let read = OperationLifecycle::from_shared(backend)
        .verified_record(intent.id())
        .unwrap();
    let same = Datum::String("owner/not-independent".into());
    assert!(matches!(
        read.validate_retained_contract(&same, &same),
        Err(OperationError::ObserverNotIndependent)
    ));
}
