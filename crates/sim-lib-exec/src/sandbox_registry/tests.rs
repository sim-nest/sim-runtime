//! Mock routing/ownership checks only; no native cleanup or M5 qualification.

use super::*;
use crate::{SandboxReport, sandbox_limit_tests};
use sim_lib_operation_gate::{
    LifecyclePreparation, OperationError, PreparedReleaseAdmission, ReleaseAction,
    ReleaseDisposition,
};
use std::sync::Mutex;

mod acknowledgement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Cancel,
    Dispose,
    Prepared,
}

struct Provider {
    id: &'static str,
    original: SandboxInvocation,
    request: SandboxRequest,
    binding: Datum,
    failure: Option<SandboxRefusal>,
    calls: Mutex<Vec<Call>>,
}

impl Provider {
    fn new(id: &'static str, failure: Option<SandboxRefusal>) -> Arc<Self> {
        let original = SandboxInvocation {
            dispatch: Datum::String("original-dispatch".into())
                .content_id()
                .unwrap(),
            command: sandbox_limit_tests::command(sandbox_limit_tests::limits()),
        };
        Arc::new(Self {
            id,
            request: original.command.sandbox_request().unwrap(),
            original,
            binding: Datum::String("original-preparation".into()),
            failure,
            calls: Mutex::new(vec![]),
        })
    }

    fn assert_calls(&self, expected: &[Call]) {
        assert_eq!(self.calls.lock().unwrap().as_slice(), expected);
    }
}

impl SandboxLauncher for Provider {
    fn id(&self) -> &str {
        self.id
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        panic!("prepared routing must not call direct launch")
    }

    fn launch_prepared(
        &self,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        binding: &Datum,
        cancellation: &ProcessCancellation,
        _: &mut dyn PreparedReleaseAdmission,
    ) -> SandboxAttempt {
        assert!(!cancellation.is_cancelled());
        assert_eq!(invocation, &self.original);
        assert_eq!(request, &self.request);
        assert_eq!(binding, &self.binding);
        self.calls.lock().unwrap().push(Call::Prepared);
        SandboxAttempt::Refused(unavailable(self.id, "original prepared route marker"))
    }

    fn cancel_prepared(
        &self,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        binding: &Datum,
        cancellation: &ProcessCancellation,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        assert!(cancellation.is_cancelled());
        self.calls.lock().unwrap().push(Call::Cancel);
        // The registry knows projection validity, not the retained original
        // dispatch or binding. Its selected owner must check those before any
        // disposal. This fixture represents that ownership distinction only.
        if invocation != &self.original || request != &self.request || binding != &self.binding {
            return Err(Box::new(unavailable(
                self.id,
                "original custody binding differs",
            )));
        }
        self.calls.lock().unwrap().push(Call::Dispose);
        self.failure
            .clone()
            .map_or(Ok(()), |error| Err(Box::new(error)))
    }
}

struct Observer;

impl SandboxObserver for Observer {
    fn id(&self) -> &str {
        "independent-observer"
    }

    fn observe(
        &self,
        _: &SandboxInvocation,
        _: &SandboxRequest,
        _: &Datum,
        _: Option<&sim_kernel::ContentId>,
    ) -> std::result::Result<Box<dyn SandboxObservation>, Box<SandboxRefusal>> {
        panic!("cleanup routing must not acquire observation authority")
    }
}

struct NoAdmission;

impl PreparedReleaseAdmission for NoAdmission {
    fn preparation(&self) -> Option<&LifecyclePreparation> {
        panic!("cleanup routing must not query release admission")
    }

    fn admit(
        &mut self,
        _: &mut dyn ReleaseAction,
    ) -> std::result::Result<ReleaseDisposition, OperationError> {
        panic!("cleanup routing must not admit payload release")
    }
}

fn registered(provider: &Arc<Provider>) -> LauncherRegistry {
    let mut registry = LauncherRegistry::default();
    registry
        .register_prepared(provider.clone(), Arc::new(Observer))
        .unwrap();
    registry
}

fn cancelled() -> ProcessCancellation {
    let cancellation = ProcessCancellation::default();
    cancellation.cancel();
    cancellation
}

fn unknown(attempt: SandboxAttempt) -> SandboxRefusal {
    match attempt {
        SandboxAttempt::Unknown(refusal) => refusal,
        other => panic!("cleanup must remain Unknown, got {other:?}"),
    }
}

fn run_cancelled(registry: &LauncherRegistry, provider: &Provider) -> SandboxRefusal {
    unknown(registry.launch_prepared(
        provider.id,
        &provider.original,
        &provider.request,
        &provider.binding,
        &cancelled(),
        &mut NoAdmission,
    ))
}

#[test]
fn already_cancelled_preparation_disposes_only_original_provider_without_release() {
    let original = Provider::new("fixture", None);
    let mut registry = registered(&original);
    let replacement = Provider::new("fixture", None);
    assert!(
        registry
            .register_prepared(replacement.clone(), Arc::new(Observer))
            .is_err()
    );
    let foreign = Provider::new("foreign", None);
    registry
        .register_prepared(foreign.clone(), Arc::new(Observer))
        .unwrap();
    let refusal = run_cancelled(&registry, &original);
    assert_eq!(refusal.launcher, "fixture");
    assert!(refusal.report.is_none());
    assert!(refusal.reason.contains("no payload completion"));
    original.assert_calls(&[Call::Cancel, Call::Dispose]);
    replacement.assert_calls(&[]);
    foreign.assert_calls(&[]);
}

#[test]
fn cancelled_cleanup_error_and_partial_report_are_preserved_as_unknown() {
    let failure = SandboxRefusal {
        launcher: "fixture".into(),
        reason: "original cleanup remained uncertain".into(),
        report: Some(SandboxReport {
            launcher: "fixture".into(),
            controls: vec![],
            limit_hits: vec![],
            cleanup: "fixture diagnostic, not native proof".into(),
        }),
    };
    let provider = Provider::new("fixture", Some(failure.clone()));
    assert_eq!(run_cancelled(&registered(&provider), &provider), failure);
    provider.assert_calls(&[Call::Cancel, Call::Dispose]);
}

struct Unsupported;

impl SandboxLauncher for Unsupported {
    fn id(&self) -> &str {
        "fixture"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        panic!("unsupported cancellation cannot use direct launch")
    }

    fn launch_prepared(
        &self,
        _: &SandboxInvocation,
        _: &SandboxRequest,
        _: &Datum,
        _: &ProcessCancellation,
        _: &mut dyn PreparedReleaseAdmission,
    ) -> SandboxAttempt {
        panic!("unsupported cancellation cannot use prepared release")
    }
}

#[test]
fn default_cancel_prepared_refuses_without_an_execution_fallback() {
    let provider = Provider::new("fixture", None);
    let mut registry = LauncherRegistry::default();
    registry
        .register_prepared(Arc::new(Unsupported), Arc::new(Observer))
        .unwrap();
    let refusal = run_cancelled(&registry, &provider);
    assert_eq!(refusal.launcher, "fixture");
    assert_eq!(
        refusal.reason,
        "prepared cancellation disposition is unsupported"
    );
    assert!(refusal.report.is_none());
}

#[test]
fn unregistered_or_unprepared_route_cannot_invoke_cleanup() {
    let provider = Provider::new("fixture", None);
    let mut registry = LauncherRegistry::default();
    let refusal = run_cancelled(&registry, &provider);
    assert!(refusal.reason.contains("not registered"));
    registry.register(provider.clone()).unwrap();
    let refusal = run_cancelled(&registry, &provider);
    assert!(refusal.reason.contains("not registered"));
    provider.assert_calls(&[]);
}

#[test]
fn mismatched_command_request_and_launcher_refuse_before_provider_cleanup() {
    let provider = Provider::new("fixture", None);
    let registry = registered(&provider);
    let mut limits = sandbox_limit_tests::limits();
    limits.output_bytes -= 1;
    let substituted = SandboxInvocation {
        dispatch: provider.original.dispatch.clone(),
        command: sandbox_limit_tests::command(limits),
    };
    let substituted_request = substituted.command.sandbox_request().unwrap();
    for (id, invocation, request) in [
        ("fixture", &substituted, &provider.request),
        ("fixture", &provider.original, &substituted_request),
        ("foreign", &provider.original, &provider.request),
    ] {
        let refusal = unknown(registry.launch_prepared(
            id,
            invocation,
            request,
            &provider.binding,
            &cancelled(),
            &mut NoAdmission,
        ));
        assert_eq!(refusal.reason, "prepared command projection mismatch");
    }
    provider.assert_calls(&[]);
}

#[test]
fn substituted_valid_invocation_or_binding_reaches_owner_check_but_not_disposition() {
    let provider = Provider::new("fixture", None);
    let registry = registered(&provider);
    let mut substituted = provider.original.clone();
    substituted.dispatch = Datum::String("foreign-dispatch".into())
        .content_id()
        .unwrap();
    let foreign_binding = Datum::String("foreign-preparation".into());
    let mut limits = sandbox_limit_tests::limits();
    limits.output_bytes -= 1;
    let changed_command = SandboxInvocation {
        dispatch: provider.original.dispatch.clone(),
        command: sandbox_limit_tests::command(limits),
    };
    let changed_request = changed_command.command.sandbox_request().unwrap();
    for (invocation, request, binding) in [
        (&substituted, &provider.request, &provider.binding),
        (&provider.original, &provider.request, &foreign_binding),
        (&changed_command, &changed_request, &provider.binding),
    ] {
        let refusal = unknown(registry.launch_prepared(
            "fixture",
            invocation,
            request,
            binding,
            &cancelled(),
            &mut NoAdmission,
        ));
        assert_eq!(refusal.reason, "original custody binding differs");
    }
    provider.assert_calls(&[Call::Cancel, Call::Cancel, Call::Cancel]);
}

#[test]
fn noncancelled_original_route_keeps_prepared_dispatch_and_result() {
    let provider = Provider::new("fixture", None);
    let result = registered(&provider).launch_prepared(
        "fixture",
        &provider.original,
        &provider.request,
        &provider.binding,
        &ProcessCancellation::default(),
        &mut NoAdmission,
    );
    assert_eq!(
        result,
        SandboxAttempt::Refused(unavailable("fixture", "original prepared route marker"))
    );
    provider.assert_calls(&[Call::Prepared]);
}
