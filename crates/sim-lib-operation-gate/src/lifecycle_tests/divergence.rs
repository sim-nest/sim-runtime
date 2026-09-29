//! Idempotent divergence sealing and non-reopening regressions.

use super::*;

#[test]
fn terminal_idempotent_divergence_never_reopens_the_same_operation() {
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

    // A retry is a new operation. A persisted outcome seals this operation even
    // when its original replay policy was idempotent.
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
    // A negative observation taken at the exact instant of dispatch cannot
    // certify Diverged -- the dispatch's own lease is still live at that
    // instant by construction. This fixture's performer advances a shared
    // clock past its lease's expiry before returning, so the setup's first
    // outcome is a genuine (not merely same-tick-coincidental) Diverged; the
    // rest of this test cares only that it seals and never reopens.
    thread_local! {
        static DIVERGENCE_TICK: std::cell::Cell<u64> = const { std::cell::Cell::new(100) };
    }
    fn divergence_clock_domain() -> Datum {
        Datum::String("fixture/divergence-ticks".into())
    }
    struct DivergenceClock;
    impl LeaseClock for DivergenceClock {
        fn read(&self) -> Result<LeaseClockReading, OperationError> {
            Ok(LeaseClockReading {
                domain: divergence_clock_domain(),
                tick: DIVERGENCE_TICK.get(),
            })
        }
    }
    struct NoEffect(Arc<AtomicUsize>);
    impl LifecyclePerformer for NoEffect {
        fn identity(&self) -> Datum {
            Datum::String("performer/no-effect".into())
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            self.0.fetch_add(1, Ordering::SeqCst);
            DIVERGENCE_TICK.set(120);
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
    let retry_calls = Arc::new(AtomicUsize::new(0));
    let mut no_effect = NoEffect(retry_calls.clone());
    DIVERGENCE_TICK.set(100);
    let first = OperationLifecycle::from_shared(backend.clone())
        .with_clock(Arc::new(DivergenceClock))
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 100, 120)
                .unwrap()
                .in_clock(divergence_clock_domain())
                .unwrap(),
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
    assert!(matches!(before_expiry, OperationOutcome::Diverged { .. }));
    assert_eq!(retry_calls.load(Ordering::SeqCst), 1);
    let retained_under_unused_window = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/rewound".into()), 90, 95).unwrap(),
            &mut no_effect,
            &mut observer,
        )
        .unwrap();
    assert!(matches!(
        retained_under_unused_window,
        OperationOutcome::Diverged { .. }
    ));
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
    assert_eq!(retry_calls.load(Ordering::SeqCst), 1);
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .unwrap()
        .unwrap();
    assert_eq!(record.dispatches().len(), 1);
}
