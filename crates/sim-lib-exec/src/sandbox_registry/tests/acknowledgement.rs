//! Modeled provider routing only; no native cleanup or authority evidence.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HookCall {
    Entered,
    Disposition,
}

struct AckOwner {
    provider: Arc<Provider>,
    cause: Arc<OperationError>,
    calls: Mutex<Vec<HookCall>>,
}

impl AckOwner {
    fn new(id: &'static str, failure: Option<SandboxRefusal>) -> Arc<Self> {
        Arc::new(Self {
            provider: Provider::new(id, failure),
            cause: Arc::new(OperationError::NonCanonical("original preparation binding")),
            calls: Mutex::new(vec![]),
        })
    }

    fn assert_calls(&self, expected: &[HookCall]) {
        assert_eq!(self.calls.lock().unwrap().as_slice(), expected);
        self.provider.assert_calls(&[]);
    }
}

impl SandboxLauncher for AckOwner {
    fn id(&self) -> &str {
        self.provider.id
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        panic!("failed acknowledgement must never invoke direct launch")
    }

    fn launch_prepared(
        &self,
        _: &SandboxInvocation,
        _: &SandboxRequest,
        _: &Datum,
        _: &ProcessCancellation,
        _: &mut dyn PreparedReleaseAdmission,
    ) -> SandboxAttempt {
        panic!("failed acknowledgement must never invoke prepared release")
    }

    fn preparation_acknowledgement_failed(
        &self,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        cause: &OperationError,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        self.calls.lock().unwrap().push(HookCall::Entered);
        // This checks forwarding of the original borrowed cause, not just an
        // independently reconstructed error with an equal diagnostic string.
        assert!(std::ptr::eq(cause, self.cause.as_ref()));
        if invocation != &self.provider.original || request != &self.provider.request {
            return Err(Box::new(unavailable(
                self.id(),
                "original custody binding differs",
            )));
        }
        self.calls.lock().unwrap().push(HookCall::Disposition);
        self.provider
            .failure
            .clone()
            .map_or(Ok(()), |error| Err(Box::new(error)))
    }
}

fn registry(owner: &Arc<AckOwner>) -> LauncherRegistry {
    let mut registry = LauncherRegistry::default();
    registry
        .register_prepared(owner.clone(), Arc::new(Observer))
        .unwrap();
    registry
}

fn notify(
    registry: &LauncherRegistry,
    owner: &AckOwner,
) -> std::result::Result<(), Box<SandboxRefusal>> {
    registry.preparation_acknowledgement_failed(
        owner.id(),
        &owner.provider.original,
        &owner.provider.request,
        &owner.cause,
    )
}

#[test]
fn acknowledgement_failure_preserves_original_registered_owner_and_borrowed_cause() {
    let original = AckOwner::new("fixture", None);
    let replacement = AckOwner::new("fixture", None);
    let foreign = AckOwner::new("foreign", None);
    let mut registry = registry(&original);
    assert!(
        registry
            .register_prepared(replacement.clone(), Arc::new(Observer))
            .is_err()
    );
    registry
        .register_prepared(foreign.clone(), Arc::new(Observer))
        .unwrap();
    notify(&registry, &original).unwrap();
    original.assert_calls(&[HookCall::Entered, HookCall::Disposition]);
    replacement.assert_calls(&[]);
    foreign.assert_calls(&[]);
}

#[test]
fn acknowledgement_disposition_refusal_preserves_exact_report_without_execution() {
    let failure = SandboxRefusal {
        launcher: "fixture".into(),
        reason: "original disposition incomplete".into(),
        report: Some(SandboxReport {
            launcher: "fixture".into(),
            controls: vec![],
            limit_hits: vec![],
            cleanup: "component diagnostic only".into(),
        }),
    };
    let owner = AckOwner::new("fixture", Some(failure.clone()));
    assert_eq!(*notify(&registry(&owner), &owner).unwrap_err(), failure);
    owner.assert_calls(&[HookCall::Entered, HookCall::Disposition]);
}

#[test]
fn unregistered_and_unprepared_acknowledgement_routes_never_invoke_owner() {
    let owner = AckOwner::new("fixture", None);
    let mut registry = LauncherRegistry::default();
    for registered in [false, true] {
        if registered {
            registry.register(owner.clone()).unwrap();
        }
        assert_eq!(
            *notify(&registry, &owner).unwrap_err(),
            unavailable("fixture", "prepared launcher is not registered"),
        );
    }
    owner.assert_calls(&[]);
}

#[test]
fn substituted_projection_is_refused_before_acknowledgement_hook() {
    let owner = AckOwner::new("fixture", None);
    let registry = registry(&owner);
    let mut limits = sandbox_limit_tests::limits();
    limits.output_bytes -= 1;
    let substituted = SandboxInvocation {
        dispatch: owner.provider.original.dispatch.clone(),
        command: sandbox_limit_tests::command(limits),
    };
    let substituted_request = substituted.command.sandbox_request().unwrap();
    for (id, invocation, request) in [
        ("fixture", &substituted, &owner.provider.request),
        ("fixture", &owner.provider.original, &substituted_request),
        ("foreign", &owner.provider.original, &owner.provider.request),
    ] {
        assert_eq!(
            *registry
                .preparation_acknowledgement_failed(id, invocation, request, &owner.cause)
                .unwrap_err(),
            unavailable(id, "prepared command projection mismatch"),
        );
    }
    owner.assert_calls(&[]);
}

#[test]
fn valid_foreign_dispatch_reaches_original_owner_check_but_never_disposition() {
    let owner = AckOwner::new("fixture", None);
    let registry = registry(&owner);
    let mut substituted = owner.provider.original.clone();
    substituted.dispatch = Datum::String("foreign-dispatch".into())
        .content_id()
        .unwrap();
    let mut limits = sandbox_limit_tests::limits();
    limits.output_bytes -= 1;
    let changed = SandboxInvocation {
        dispatch: owner.provider.original.dispatch.clone(),
        command: sandbox_limit_tests::command(limits),
    };
    let changed_request = changed.command.sandbox_request().unwrap();
    for (invocation, request) in [
        (&substituted, &owner.provider.request),
        (&changed, &changed_request),
    ] {
        assert_eq!(
            *registry
                .preparation_acknowledgement_failed("fixture", invocation, request, &owner.cause,)
                .unwrap_err(),
            unavailable("fixture", "original custody binding differs"),
        );
    }
    owner.assert_calls(&[HookCall::Entered, HookCall::Entered]);
}

#[test]
fn unsupported_acknowledgement_disposition_has_no_execution_fallback() {
    let owner = AckOwner::new("fixture", None);
    let mut registry = LauncherRegistry::default();
    registry
        .register_prepared(Arc::new(Unsupported), Arc::new(Observer))
        .unwrap();
    assert_eq!(
        *notify(&registry, &owner).unwrap_err(),
        unavailable(
            "fixture",
            "original preparation failure disposition is unsupported"
        ),
    );
    owner.assert_calls(&[]);
}
