//! conformance: verified-head binding, restart projection, and non-conflated outcomes.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use sim_kernel::{CapabilityName, Datum};
use sim_lib_journal::MemoryBackend;
use sim_lib_operation_gate::{
    CancellationSignal, ContractVerifiedOperation, FencedDispatch, LeaseClock, LeaseClockReading,
    LeaseWindow, LifecyclePerformer, LifecyclePerformerResponse, OperationGrant, OperationIntent,
    OperationLifecycle, OperationOutcome, OperationStep, PostconditionObserver,
    PostconditionRequest, PostconditionResponse,
};

use crate::{
    ArgAtom, BuildSourceRef, CapabilityGrantRef, CleanupContract, CommandInvocation,
    CommandReplayPolicy, CommandResource, CommandRoute, CommandSpec, LocalCheckDurableState,
    LocalCheckReconciliation, LocalCheckRequest, LocalCheckStatus, NetworkAccess, OutputContract,
    OutputExpectation, OutputState, PacketRef, ProcessBudget, ProgramRef, ProjectRootRef,
    ResourceAccess, SealedBindings, local_check_intent,
};

fn request() -> (LocalCheckRequest, CommandSpec) {
    let command = CommandSpec::new(
        ProgramRef::new("shell").unwrap(),
        ProjectRootRef::new("checkout").unwrap(),
        CommandInvocation::Interpreter {
            flags: vec![ArgAtom::new("-c").unwrap()],
            script: b"cargo test -p tiny".to_vec(),
        },
        SealedBindings::literals([("PATH".into(), "/toolchain/bin".into())]).unwrap(),
        vec![CommandResource {
            source: "out".into(),
            guest_path: "/out".into(),
            access: ResourceAccess::Writable,
        }],
        ProcessBudget {
            timeout_ms: 1_000,
            max_output_bytes: 4_096,
            stdin: None,
        },
        OutputContract::new(
            [0],
            vec![OutputExpectation {
                resource: "out".into(),
                relative_path: "result".into(),
                state: OutputState::Exists,
            }],
        )
        .unwrap(),
        CleanupContract::process_group(["out".into()]).unwrap(),
        NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
        CommandRoute::Process,
        CommandReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    let request = LocalCheckRequest::new(
        PacketRef::new("packet/exact").unwrap(),
        command.id().clone(),
        BuildSourceRef::new("source/sealed").unwrap(),
        CapabilityGrantRef::new("grant/scoped").unwrap(),
    );
    (request, command)
}

fn operation(
    request: &LocalCheckRequest,
    command: &CommandSpec,
) -> (OperationIntent, OperationGrant) {
    let intent = local_check_intent(request, command).unwrap();
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("local-check/run"),
        Datum::String("authority/installed".into()),
    )
    .unwrap();
    (intent, grant)
}

#[derive(Default)]
struct Stop(AtomicBool);

impl CancellationSignal for Stop {
    fn request_stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn contract_verified<P, O>(
    backend: Arc<MemoryBackend>,
    intent: &OperationIntent,
    grant: &OperationGrant,
    performer: P,
    observer: O,
) -> ContractVerifiedOperation
where
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    OperationLifecycle::from_shared(backend)
        .accept(
            intent,
            grant,
            LeaseWindow::new(Datum::String("holder/retained-owner".into()), 30, 40).unwrap(),
        )
        .unwrap()
        .retain_custody(performer, observer, Arc::new(Stop::default()))
        .unwrap()
        .verify_retained_contract()
        .unwrap()
}

struct NeverPerforms;

impl LifecyclePerformer for NeverPerforms {
    fn identity(&self) -> Datum {
        Datum::String("performer/never".into())
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        panic!("unavailable preflight must not dispatch")
    }
}

struct UnavailableObserver;

impl PostconditionObserver for UnavailableObserver {
    fn identity(&self) -> Datum {
        Datum::String("observer/independent".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        PostconditionResponse::Unavailable {
            reason: Datum::String("observer/unavailable".into()),
        }
    }
}

thread_local! { static CLOCK_TICK: std::cell::Cell<u64> = const { std::cell::Cell::new(10) }; }

fn shared_tick_domain() -> Datum {
    Datum::String("fixture/shared-ticks".into())
}

struct SharedTickClock;

impl LeaseClock for SharedTickClock {
    fn read(&self) -> Result<LeaseClockReading, sim_lib_operation_gate::OperationError> {
        Ok(LeaseClockReading {
            domain: shared_tick_domain(),
            tick: CLOCK_TICK.get(),
        })
    }
}

/// Performs with no effect, but advances the shared clock past its own lease's
/// expiry first -- simulating a negative observation genuinely taken after the
/// attempt's window elapsed, not merely at the instant of dispatch.
struct ExpiringNoEffect;

impl LifecyclePerformer for ExpiringNoEffect {
    fn identity(&self) -> Datum {
        Datum::String("performer/no-effect-expiring".into())
    }

    fn perform(&mut self, _dispatch: &FencedDispatch) -> LifecyclePerformerResponse {
        // No receipt: a durably gated receipt commit would itself refuse once
        // the lease is dead (the direct-perform path's own round-2 guard),
        // short-circuiting before the observation this test wants to exercise
        // ever runs. A lost acknowledgement is the realistic way a live
        // dispatch's postcondition ends up observed only after its lease
        // expired.
        CLOCK_TICK.set(20);
        LifecyclePerformerResponse::AcknowledgementMissing
    }
}

struct NegativeObserver;

impl PostconditionObserver for NegativeObserver {
    fn identity(&self) -> Datum {
        Datum::String("observer/negative".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        PostconditionResponse::NotSatisfied {
            observed: Datum::String("not-satisfied".into()),
            evidence: Datum::String("observer/negative-evidence".into()),
        }
    }
}

struct EffectPerformer(Arc<AtomicBool>);

impl LifecyclePerformer for EffectPerformer {
    fn identity(&self) -> Datum {
        Datum::String("performer/effect".into())
    }

    fn perform(&mut self, dispatch: &FencedDispatch) -> LifecyclePerformerResponse {
        self.0.store(true, Ordering::SeqCst);
        LifecyclePerformerResponse::Receipt(Datum::String(dispatch.id().to_string()))
    }
}

struct EffectObserver(Arc<AtomicBool>);

impl PostconditionObserver for EffectObserver {
    fn identity(&self) -> Datum {
        Datum::String("observer/effect".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        if self.0.load(Ordering::SeqCst) {
            PostconditionResponse::Satisfied {
                observed: Datum::String("satisfied".into()),
                evidence: Datum::String("observer/satisfied-evidence".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::String("not-satisfied".into()),
                evidence: Datum::String("observer/not-satisfied-evidence".into()),
            }
        }
    }
}

#[test]
fn accepted_without_outcome_is_pending_after_reopen_not_refused() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (intent, grant) = operation(&request, &command);
    let accepted = OperationLifecycle::from_shared(backend.clone())
        .accept(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
        )
        .unwrap();
    drop(accepted);

    let retained = contract_verified(
        backend.clone(),
        &intent,
        &grant,
        NeverPerforms,
        UnavailableObserver,
    );
    let projected =
        LocalCheckReconciliation::from_contract_verified_operation(&request, &command, &retained)
            .unwrap();
    let LocalCheckDurableState::Pending(pending) = projected.state() else {
        panic!("accepted operation without outcome must remain pending")
    };
    assert_eq!(pending.last_step(), OperationStep::IntentPersisted);
    assert!(!pending.dispatch_seen());
    assert!(!pending.cancellation_requested());
    assert!(projected.journal_head().is_some());

    let reopened_retained =
        contract_verified(backend, &intent, &grant, NeverPerforms, UnavailableObserver);
    let reopened = LocalCheckReconciliation::from_contract_verified_operation(
        &request,
        &command,
        &reopened_retained,
    )
    .unwrap();
    assert_eq!(reopened, projected);
    assert_eq!(
        reopened.content_id().unwrap(),
        projected.content_id().unwrap()
    );
}

#[test]
fn a_retained_owner_cannot_project_an_unaccepted_operation_as_absent() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (existing, grant) = operation(&request, &command);
    let missing_command = command.clone();
    let missing_request = LocalCheckRequest::new(
        PacketRef::new("packet/missing").unwrap(),
        missing_command.id().clone(),
        BuildSourceRef::new("source/sealed").unwrap(),
        CapabilityGrantRef::new("grant/scoped").unwrap(),
    );
    let (missing, missing_grant) = operation(&missing_request, &missing_command);
    let accepted = OperationLifecycle::from_shared(backend.clone())
        .accept(
            &existing,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
        )
        .unwrap();
    drop(accepted);

    let retained = contract_verified(
        backend,
        &missing,
        &missing_grant,
        NeverPerforms,
        UnavailableObserver,
    );
    let projected = LocalCheckReconciliation::from_contract_verified_operation(
        &missing_request,
        &missing_command,
        &retained,
    )
    .unwrap();
    assert!(projected.journal_head().is_some());
    assert!(matches!(
        projected.state(),
        LocalCheckDurableState::Pending(_)
    ));
}

#[test]
fn durable_uncertainty_is_completed_and_never_api_refusal() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (intent, grant) = operation(&request, &command);
    let outcome = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut NeverPerforms,
            &mut UnavailableObserver,
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Uncertain { .. }));

    let read = OperationLifecycle::from_shared(backend.clone())
        .verified_record(intent.id())
        .unwrap();
    let retained = contract_verified(backend, &intent, &grant, NeverPerforms, UnavailableObserver);
    let projected =
        LocalCheckReconciliation::from_contract_verified_operation(&request, &command, &retained)
            .unwrap();
    let LocalCheckDurableState::Completed(receipt) = projected.state() else {
        panic!("durable uncertainty is a completed lifecycle outcome")
    };
    assert_eq!(receipt.status(), LocalCheckStatus::Uncertain);
    assert_ne!(receipt.status(), LocalCheckStatus::Refused);
    assert_eq!(
        receipt.outcome(),
        read.record().unwrap().outcome_id().unwrap()
    );
}

#[test]
fn durable_negative_observation_projects_diverged_not_pending_or_refused() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (intent, grant) = operation(&request, &command);
    CLOCK_TICK.set(10);
    let outcome = OperationLifecycle::from_shared(backend.clone())
        .with_clock(Arc::new(SharedTickClock))
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20)
                .unwrap()
                .in_clock(shared_tick_domain())
                .unwrap(),
            &mut ExpiringNoEffect,
            &mut NegativeObserver,
        )
        .unwrap();
    // A negative observation taken while the dispatch's own lease is still
    // live cannot be sealed Diverged: the performer may still complete. This
    // fixture's performer advances the clock past that lease's expiry before
    // returning, so the observation genuinely postdates it and Diverged is
    // reached honestly, not merely because no clock was configured to notice
    // the difference.
    assert!(matches!(outcome, OperationOutcome::Diverged { .. }));

    let read = OperationLifecycle::from_shared(backend.clone())
        .verified_record(intent.id())
        .unwrap();
    let retained = contract_verified(backend, &intent, &grant, ExpiringNoEffect, NegativeObserver);
    let projected =
        LocalCheckReconciliation::from_contract_verified_operation(&request, &command, &retained)
            .unwrap();
    let LocalCheckDurableState::Completed(receipt) = projected.state() else {
        panic!("durable negative outcome cannot become pending")
    };
    assert_eq!(receipt.status(), LocalCheckStatus::Diverged);
    assert_ne!(receipt.status(), LocalCheckStatus::Refused);
    assert_eq!(
        receipt.outcome(),
        read.record().unwrap().outcome_id().unwrap()
    );
}

#[test]
fn independently_verified_outcome_projects_a_head_bound_receipt() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (intent, grant) = operation(&request, &command);
    let effect = Arc::new(AtomicBool::new(false));
    let outcome = OperationLifecycle::from_shared(backend.clone())
        .run(
            &intent,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
            &mut EffectPerformer(effect.clone()),
            &mut EffectObserver(effect),
        )
        .unwrap();
    assert!(matches!(outcome, OperationOutcome::Verified { .. }));

    let read = OperationLifecycle::from_shared(backend.clone())
        .verified_record(intent.id())
        .unwrap();
    let retained = contract_verified(
        backend,
        &intent,
        &grant,
        EffectPerformer(Arc::new(AtomicBool::new(true))),
        EffectObserver(Arc::new(AtomicBool::new(true))),
    );
    let projected =
        LocalCheckReconciliation::from_contract_verified_operation(&request, &command, &retained)
            .unwrap();
    let LocalCheckDurableState::Completed(receipt) = projected.state() else {
        panic!("verified durable outcome must produce a receipt")
    };
    assert_eq!(receipt.status(), LocalCheckStatus::Verified);
    assert!(projected.journal_head().is_some());
    assert_eq!(
        receipt.outcome(),
        read.record().unwrap().outcome_id().unwrap()
    );
    assert!(projected.content_id().is_ok());
}

#[test]
fn foreign_operation_cannot_be_spliced_onto_a_verified_record() {
    let backend = Arc::new(MemoryBackend::new());
    let (request, command) = request();
    let (recorded, grant) = operation(&request, &command);
    let accepted = OperationLifecycle::from_shared(backend.clone())
        .accept(
            &recorded,
            &grant,
            LeaseWindow::new(Datum::String("holder/a".into()), 10, 20).unwrap(),
        )
        .unwrap();
    drop(accepted);
    let substituted = LocalCheckRequest::new(
        PacketRef::new("packet/substituted").unwrap(),
        command.id().clone(),
        BuildSourceRef::new("source/sealed").unwrap(),
        CapabilityGrantRef::new("grant/scoped").unwrap(),
    );
    let retained = contract_verified(
        backend,
        &recorded,
        &grant,
        NeverPerforms,
        UnavailableObserver,
    );
    assert!(
        LocalCheckReconciliation::from_contract_verified_operation(
            &substituted,
            &command,
            &retained,
        )
        .is_err()
    );
}
