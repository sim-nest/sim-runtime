use crate::*;
use sim_kernel::{Error, Expr, Symbol, testing::bare_cx};
use std::sync::Mutex;

#[derive(Default)]
struct RecordingPort {
    requests: Mutex<Vec<ProcessRequest>>,
    outcome: Mutex<Option<ProcessAttempt>>,
}
impl ProcessPort for RecordingPort {
    fn run(&self, request: &ProcessRequest, _: &ProcessCancellation) -> ProcessAttempt {
        self.requests.lock().unwrap().push(request.clone());
        self.outcome
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ProcessAttempt::Completed {
                receipt: ProcessReceipt {
                    provider: "model".into(),
                    elapsed_mono_ns: 7,
                    result: ProcResult {
                        stdout: "out".into(),
                        stderr: String::new(),
                        exit_code: 0,
                        truncated: false,
                    },
                },
            })
    }
}
fn options() -> ExecOptions {
    ExecOptions::new(
        ProgramRef::new("printf").unwrap(),
        ProjectRootRef::new("project").unwrap(),
        100,
        16,
    )
}
fn argv() -> Vec<String> {
    vec!["hello world".into()]
}

#[test]
fn capability_and_policy_precede_dispatch() {
    let port = RecordingPort::default();
    let mut cx = bare_cx();
    let denied = exec(
        &mut cx,
        &port,
        &argv(),
        &options(),
        &ProcessCancellation::default(),
    )
    .unwrap_err();
    assert!(matches!(denied,Error::CapabilityDenied{capability} if capability==exec_capability()));
    cx.grant(exec_capability());
    for mut opts in [options(), options()] {
        if opts.budget.timeout_ms == 100 {
            opts.budget.timeout_ms = 0
        } else {
            opts.budget.max_output_bytes = 0
        }
        assert!(
            exec(
                &mut cx,
                &port,
                &argv(),
                &opts,
                &ProcessCancellation::default()
            )
            .is_err()
        )
    }
    assert!(port.requests.lock().unwrap().is_empty())
}

#[test]
fn request_is_opaque_whole_atom_and_empty_by_default() {
    let port = RecordingPort::default();
    let mut cx = bare_cx();
    cx.grant(exec_capability());
    exec(
        &mut cx,
        &port,
        &argv(),
        &options().with_stdin(b"input".to_vec()),
        &ProcessCancellation::default(),
    )
    .unwrap();
    let requests = port.requests.lock().unwrap();
    let request = &requests[0];
    assert_eq!(request.program.as_str(), "printf");
    assert_eq!(request.root.as_str(), "project");
    assert_eq!(request.argv[0].as_str(), "hello world");
    assert_eq!(request.environment.iter().count(), 0)
}

#[test]
fn sealed_bindings_validate_names_nul_duplicates_bounds_and_kinds() {
    assert!(
        SealedBindings::try_from_entries([("BAD=NAME".into(), BindingValue::Literal("x".into()))])
            .is_err()
    );
    assert!(SealedBindings::literals([("A".into(), "x\0y".into())]).is_err());
    assert!(
        SealedBindings::try_from_entries([
            ("A".into(), BindingValue::Literal("1".into())),
            ("A".into(), BindingValue::Literal("2".into()))
        ])
        .is_err()
    );
    let huge = "x".repeat(65 * 1024);
    assert!(SealedBindings::literals([("A".into(), huge)]).is_err());
    let bindings = SealedBindings::try_from_entries([
        (
            "ROOT".into(),
            BindingValue::ProjectRoot(ProjectRootRef::new("project").unwrap()),
        ),
        (
            "SECRET".into(),
            BindingValue::PrivateArtifact(PrivateArtifactRef::new("token").unwrap()),
        ),
    ])
    .unwrap();
    assert_eq!(bindings.iter().count(), 2)
}

#[test]
fn only_not_dispatched_is_retryable() {
    let attempts = [
        ProcessAttempt::NotDispatched {
            refusal: ProcessRefusal::SpawnFailed("missing".into()),
        },
        ProcessAttempt::Completed {
            receipt: ProcessReceipt {
                provider: "m".into(),
                elapsed_mono_ns: 0,
                result: ProcResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 7,
                    truncated: false,
                },
            },
        },
        ProcessAttempt::StoppedAfterTimeout {
            receipt: StopReceipt {
                provider: "m".into(),
                elapsed_mono_ns: 1,
                cleanup: "reaped".into(),
            },
        },
        ProcessAttempt::StoppedAfterCancel {
            receipt: StopReceipt {
                provider: "m".into(),
                elapsed_mono_ns: 1,
                cleanup: "reaped".into(),
            },
        },
        ProcessAttempt::UnknownAfterDispatch {
            evidence: DispatchEvidence {
                provider: "m".into(),
                stage: "reap".into(),
                detail: "unknown".into(),
            },
        },
    ];
    assert!(attempts[0].automatically_retryable());
    assert!(attempts[1..].iter().all(|v| !v.automatically_retryable()))
}

#[test]
fn cancellation_token_is_shareable() {
    let token = ProcessCancellation::default();
    let other = token.clone();
    token.cancel();
    assert!(other.is_cancelled())
}
#[test]
fn proc_result_encodes_constructor_form() {
    let result = ProcResult {
        stdout: "out".into(),
        stderr: "err".into(),
        exit_code: 7,
        truncated: true,
    };
    let Expr::Call { operator, args } = result.to_constructor_expr() else {
        panic!()
    };
    assert_eq!(*operator, Expr::Symbol(Symbol::new("ProcResult")));
    assert_eq!(args[3], Expr::Bool(true))
}
#[test]
fn portable_crate_contains_no_host_binding() {
    let source = include_str!("exec.rs");
    for forbidden in [
        "PathBuf",
        "std::path",
        "std::process",
        "std::env",
        "Instant::now",
        "Command::new",
    ] {
        assert!(!source.contains(forbidden), "runtime retained {forbidden}")
    }
}
