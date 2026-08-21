use crate::*;
use sim_kernel::{Error, Expr, Symbol, testing::bare_cx};
use std::{path::PathBuf, sync::Mutex};

// conformance: process policy is checked before an injected platform port runs.

#[derive(Default)]
struct RecordingPort {
    requests: Mutex<Vec<ProcessRequest>>,
    outcome: Mutex<Option<std::result::Result<ProcessReceipt, ProcessError>>>,
}
impl ProcessPort for RecordingPort {
    fn run(
        &self,
        request: &ProcessRequest,
        _: &ProcessCancellation,
    ) -> std::result::Result<ProcessReceipt, ProcessError> {
        self.requests.lock().unwrap().push(request.clone());
        self.outcome.lock().unwrap().take().unwrap_or_else(|| {
            Ok(ProcessReceipt {
                provider: "model".into(),
                elapsed_mono_ns: 7,
                result: ProcResult {
                    stdout: "out".into(),
                    stderr: String::new(),
                    exit_code: 0,
                    truncated: false,
                },
            })
        })
    }
}
fn options() -> ExecOptions {
    ExecOptions::new(PathBuf::from("/sandbox"), 100, 16)
}
fn argv() -> Vec<String> {
    vec!["printf".into(), "hello world".into()]
}

#[test]
fn capability_and_policy_precede_port_dispatch() {
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
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == exec_capability())
    );
    assert!(port.requests.lock().unwrap().is_empty());
    cx.grant(exec_capability());
    for (args, opts) in [
        (vec![], options()),
        (argv(), ExecOptions::new("/sandbox", 0, 16)),
        (argv(), ExecOptions::new("/sandbox", 1, 0)),
        (argv(), ExecOptions::new("relative", 1, 1)),
    ] {
        assert!(
            exec(
                &mut cx,
                &port,
                &args,
                &opts,
                &ProcessCancellation::default()
            )
            .is_err()
        );
    }
    assert!(port.requests.lock().unwrap().is_empty());
}

#[test]
fn checked_request_is_structured_bounded_rooted_and_environment_diminished() {
    let port = RecordingPort::default();
    let mut cx = bare_cx();
    cx.grant(exec_capability());
    let opts = options()
        .with_cwd("/sandbox/work")
        .with_stdin(b"input".to_vec())
        .with_env("LANG", "C");
    let result = exec(
        &mut cx,
        &port,
        &argv(),
        &opts,
        &ProcessCancellation::default(),
    )
    .unwrap();
    assert_eq!(result.stdout, "out");
    let requests = port.requests.lock().unwrap();
    let request = &requests[0];
    assert_eq!(request.argv, argv());
    assert_eq!(request.root, PathBuf::from("/sandbox"));
    assert_eq!(request.cwd, PathBuf::from("/sandbox/work"));
    assert_eq!(request.environment.len(), 1);
    assert_eq!(request.environment["LANG"], "C");
}

#[test]
fn timeout_cancellation_kill_failure_and_leak_evidence_are_preserved() {
    for (outcome, words) in [
        (
            ProcessError::Timeout {
                kill_failure: Some("denied".into()),
                leaked_descendants: true,
            },
            ["timed out", "kill failed", "leaked"],
        ),
        (
            ProcessError::Cancelled {
                kill_failure: None,
                leaked_descendants: false,
            },
            ["cancelled", "", ""],
        ),
    ] {
        let port = RecordingPort {
            requests: Mutex::default(),
            outcome: Mutex::new(Some(Err(outcome))),
        };
        let mut cx = bare_cx();
        cx.grant(exec_capability());
        let message = exec(
            &mut cx,
            &port,
            &argv(),
            &options(),
            &ProcessCancellation::default(),
        )
        .unwrap_err()
        .to_string();
        for word in words.into_iter().filter(|word| !word.is_empty()) {
            assert!(message.contains(word));
        }
    }
}

#[test]
fn cancellation_token_is_shareable() {
    let token = ProcessCancellation::default();
    let other = token.clone();
    token.cancel();
    assert!(other.is_cancelled());
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
        panic!("expected constructor")
    };
    assert_eq!(*operator, Expr::Symbol(Symbol::new("ProcResult")));
    assert_eq!(args[3], Expr::Bool(true));
}

#[test]
fn runtime_contains_no_concrete_process_mechanics() {
    let source = include_str!("exec.rs");
    for forbidden in [
        "std::process",
        "Command::new",
        "Instant::now",
        "current_dir",
        "process_group",
    ] {
        assert!(!source.contains(forbidden), "runtime retained {forbidden}");
    }
}
