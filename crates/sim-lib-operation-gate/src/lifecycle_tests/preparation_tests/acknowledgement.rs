//! Component journal/owner routing laws, not native disposition qualification.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Crash(&'static str),
    NonCanonical,
}

impl Failure {
    fn of(error: &OperationError) -> Self {
        match error {
            OperationError::Journal(JournalError::InjectedCrash(boundary)) => Self::Crash(boundary),
            OperationError::NonCanonical("semantic datum") => Self::NonCanonical,
            other => panic!("unexpected acknowledgement cause: {other:?}"),
        }
    }
}

fn crash_failure(after_commit: bool) -> Failure {
    Failure::Crash(if after_commit {
        "after M5 boundary"
    } else {
        "before M5 boundary"
    })
}

pub(super) fn assert_default_disposition(error: &OperationError, after_commit: bool) {
    let OperationError::PreparationDisposition { cause, disposition } = error else {
        panic!("missing original cause and default disposition refusal: {error:?}");
    };
    assert_eq!(Failure::of(cause), crash_failure(after_commit));
    assert!(matches!(
        disposition.as_ref(),
        OperationError::PreparationUnavailable(Datum::String(reason))
            if reason == "original preparation failure disposition is unsupported"
    ));
}

#[derive(Clone, Copy)]
enum Binding {
    Valid,
    Malformed,
    Refused,
}

struct Owner {
    inner: PreparedPerformer,
    reserved: bool,
    binding: Binding,
    fail_disposition: bool,
    original: Option<FencedDispatch>,
    calls: Vec<(FencedDispatch, Failure, usize)>,
}

impl Owner {
    fn new(inner: PreparedPerformer, reserved: bool) -> Self {
        Self {
            inner,
            reserved,
            binding: Binding::Valid,
            fail_disposition: false,
            original: None,
            calls: vec![],
        }
    }

    fn binding(&mut self, dispatch: &FencedDispatch) -> Result<Datum, OperationError> {
        assert!(self.original.replace(dispatch.clone()).is_none());
        let binding = self.inner.prepare(dispatch)?.unwrap();
        match self.binding {
            Binding::Valid => Ok(binding),
            Binding::Malformed => Ok(Datum::Set(vec![Datum::Nil, Datum::Nil])),
            Binding::Refused => Err(OperationError::PreparationUnavailable(Datum::String(
                "original prepare failed before acknowledgement".into(),
            ))),
        }
    }

    fn assert_unreleased(&self) {
        assert_eq!(self.inner.preparations.load(Ordering::SeqCst), 1);
        assert_eq!(self.inner.releases.load(Ordering::SeqCst), 0);
        assert!(!self.inner.effect.load(Ordering::SeqCst));
    }
}

impl LifecyclePerformer for Owner {
    fn identity(&self) -> Datum {
        self.inner.identity()
    }

    fn plan_reservation(&self, _: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
        Ok(self
            .reserved
            .then(|| Datum::String("fixture/reserved-destination".into())))
    }

    fn prepare(&mut self, dispatch: &FencedDispatch) -> Result<Option<Datum>, OperationError> {
        assert!(!self.reserved);
        self.binding(dispatch).map(Some)
    }

    fn prepare_reserved(
        &mut self,
        dispatch: &FencedDispatch,
        reservation: &crate::LifecycleReservation,
    ) -> Result<Datum, OperationError> {
        assert!(self.reserved);
        assert_eq!(reservation.dispatch(), dispatch.id());
        self.binding(dispatch)
    }

    fn preparation_acknowledgement_failed(
        &mut self,
        dispatch: &FencedDispatch,
        cause: &OperationError,
    ) -> Result<(), OperationError> {
        assert_eq!(Some(dispatch), self.original.as_ref());
        self.assert_unreleased();
        let record = prepared_lifecycle(self.inner.backend.clone())
            .record(dispatch.operation())?
            .unwrap();
        assert_eq!(record.dispatches().last(), Some(dispatch));
        assert!(record.releases().is_empty());
        assert!(record.receipts().is_empty());
        self.calls.push((
            dispatch.clone(),
            Failure::of(cause),
            record.preparations().len(),
        ));
        if self.fail_disposition {
            Err(OperationError::PreparationUnavailable(Datum::String(
                "original owner's disposition failed".into(),
            )))
        } else {
            Ok(())
        }
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("acknowledgement failure must never fall back to direct execution")
    }

    fn perform_prepared(
        &mut self,
        dispatch: &FencedDispatch,
        preparation: &LifecyclePreparation,
        admission: &mut dyn PreparedReleaseAdmission,
    ) -> Result<LifecyclePerformerResponse, OperationError> {
        assert!(
            self.calls.is_empty(),
            "failed acknowledgement cannot release"
        );
        self.inner
            .perform_prepared(dispatch, preparation, admission)
    }
}

fn assert_failed_disposition(error: &OperationError, expected: Failure) {
    let OperationError::PreparationDisposition { cause, disposition } = error else {
        panic!("disposition failure replaced the original cause: {error:?}");
    };
    assert_eq!(Failure::of(cause), expected);
    assert!(matches!(
        disposition.as_ref(),
        OperationError::PreparationUnavailable(Datum::String(reason))
            if reason == "original owner's disposition failed"
    ));
}

#[test]
fn preparation_journal_cuts_notify_original_owner_without_release_or_error_replacement() {
    for reserved in [false, true] {
        for after_commit in [false, true] {
            for fail_disposition in [false, true] {
                let backend = Arc::new(CrashBackend::default());
                backend.crash(5 + usize::from(reserved), after_commit);
                let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
                let (inner, mut observer) = prepared_ports(backend.clone());
                let mut owner = Owner::new(inner, reserved);
                owner.fail_disposition = fail_disposition;
                let error = prepared_lifecycle(backend)
                    .run(
                        &intent,
                        &grant,
                        clock_window("holder/a", 10, 20),
                        &mut owner,
                        &mut observer,
                    )
                    .unwrap_err();
                let expected = crash_failure(after_commit);
                if fail_disposition {
                    assert_failed_disposition(&error, expected);
                } else {
                    assert_eq!(Failure::of(&error), expected);
                }
                assert_eq!(
                    owner.calls.as_slice(),
                    &[(
                        owner.original.clone().unwrap(),
                        expected,
                        usize::from(after_commit),
                    )]
                );
                owner.assert_unreleased();
            }
        }
    }
}

#[test]
fn malformed_returned_binding_notifies_owner_with_exact_construction_error() {
    for reserved in [false, true] {
        for fail_disposition in [false, true] {
            let backend = Arc::new(CrashBackend::default());
            let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
            let (inner, mut observer) = prepared_ports(backend.clone());
            let mut owner = Owner::new(inner, reserved);
            owner.binding = Binding::Malformed;
            owner.fail_disposition = fail_disposition;
            let error = prepared_lifecycle(backend)
                .run(
                    &intent,
                    &grant,
                    clock_window("holder/a", 10, 20),
                    &mut owner,
                    &mut observer,
                )
                .unwrap_err();
            if fail_disposition {
                assert_failed_disposition(&error, Failure::NonCanonical);
            } else {
                assert_eq!(Failure::of(&error), Failure::NonCanonical);
            }
            assert_eq!(
                owner.calls.as_slice(),
                &[(owner.original.clone().unwrap(), Failure::NonCanonical, 0,)]
            );
            owner.assert_unreleased();
        }
    }
}

#[test]
fn successful_preparation_and_release_never_invoke_failure_disposition() {
    for reserved in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let (inner, mut observer) = prepared_ports(backend.clone());
        let mut owner = Owner::new(inner, reserved);
        let outcome = prepared_lifecycle(backend)
            .run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut owner,
                &mut observer,
            )
            .unwrap();
        assert!(matches!(outcome, OperationOutcome::Verified { .. }));
        assert_eq!(owner.inner.preparations.load(Ordering::SeqCst), 1);
        assert_eq!(owner.inner.releases.load(Ordering::SeqCst), 1);
        assert!(owner.calls.is_empty());
    }
}

#[test]
fn preparation_error_is_not_a_returned_binding_and_never_invokes_disposition() {
    for reserved in [false, true] {
        let backend = Arc::new(CrashBackend::default());
        let (intent, grant) = fixture(ReplayPolicy::ExactlyOnce);
        let (inner, mut observer) = prepared_ports(backend.clone());
        let mut owner = Owner::new(inner, reserved);
        owner.binding = Binding::Refused;
        let error = prepared_lifecycle(backend)
            .run(
                &intent,
                &grant,
                clock_window("holder/a", 10, 20),
                &mut owner,
                &mut observer,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            OperationError::PreparationUnavailable(Datum::String(reason))
                if reason == "original prepare failed before acknowledgement"
        ));
        assert!(owner.calls.is_empty());
        owner.assert_unreleased();
    }
}
