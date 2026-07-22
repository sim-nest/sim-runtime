use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// conformance: host exec primitive runs only through explicit capability checks.

use sim_kernel::{Error, Expr, Symbol, testing::bare_cx};

use crate::{ExecOptions, ProcResult, exec, exec_capability, proc_result_symbol};

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn options() -> ExecOptions {
    ExecOptions::new(1_000, 1_024)
}

#[test]
fn denied_capability_refuses_before_spawn() {
    let mut cx = bare_cx();
    let err = exec(
        &mut cx,
        &argv(&["sim-lib-exec-definitely-missing-command"]),
        &options(),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == exec_capability()
    ));
}

#[test]
fn empty_argv_is_rejected_before_spawn() {
    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let err = exec(&mut cx, &[], &options()).unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message.contains("non-empty argv")));
}

#[test]
fn exit_code_stdout_and_stderr_are_surfaced() {
    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let result = exec(
        &mut cx,
        &argv(&["env", "sh", "-c", "printf out; printf err >&2; exit 7"]),
        &options(),
    )
    .unwrap();

    assert_eq!(result.stdout, "out");
    assert_eq!(result.stderr, "err");
    assert_eq!(result.exit_code, 7);
    assert!(!result.truncated);
}

#[test]
fn output_cap_truncates_and_flags() {
    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let result = exec(
        &mut cx,
        &argv(&["env", "printf", "1234567890"]),
        &ExecOptions::new(1_000, 4),
    )
    .unwrap();

    assert_eq!(result.stdout, "1234");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
    assert!(result.truncated);
}

#[test]
fn timeout_kills_and_reports() {
    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let err = exec(
        &mut cx,
        &argv(&["env", "sleep", "2"]),
        &ExecOptions::new(50, 1_024),
    )
    .unwrap_err();

    assert!(matches!(err, Error::HostError(message) if message.contains("timed out")));
}

#[test]
fn timeout_also_bounds_inherited_output_pipes() {
    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let started = Instant::now();
    let err = exec(
        &mut cx,
        &argv(&["env", "sh", "-c", "sleep 1 & printf done"]),
        &ExecOptions::new(50, 1_024),
    )
    .unwrap_err();

    assert!(matches!(err, Error::HostError(message) if message.contains("timed out")));
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[cfg(unix)]
#[test]
fn timeout_kills_background_children_in_the_same_process_group() {
    let pid_dir = temp_dir("timeout-group");
    let pid_file = pid_dir.join("child.pid");

    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let err = exec(
        &mut cx,
        &argv(&[
            "env",
            "sh",
            "-c",
            "sleep 5 & echo $! > \"$1\"; wait",
            "sh",
            pid_file.to_str().unwrap(),
        ]),
        &ExecOptions::new(100, 1_024),
    )
    .unwrap_err();

    assert!(matches!(err, Error::HostError(message) if message.contains("timed out")));

    let pid = wait_for_pid(&pid_file);
    let deadline = Instant::now() + Duration::from_secs(1);
    while process_is_alive(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    if process_is_alive(pid) {
        force_kill(pid);
        panic!("background child {pid} survived exec timeout");
    }

    let _ = fs::remove_dir_all(pid_dir);
}

#[test]
fn cwd_is_confined_to_root() {
    let root = temp_dir("root");
    let child = root.join("child");
    let outside = temp_dir("outside");
    fs::create_dir_all(&child).unwrap();

    let mut cx = bare_cx();
    cx.grant(exec_capability());

    let result = exec(
        &mut cx,
        &argv(&["env", "pwd"]),
        &ExecOptions::new(1_000, 1_024).with_cwd(&child, &root),
    )
    .unwrap();
    assert_eq!(
        PathBuf::from(result.stdout.trim()),
        child.canonicalize().unwrap()
    );

    let err = exec(
        &mut cx,
        &argv(&["env", "pwd"]),
        &ExecOptions::new(1_000, 1_024).with_cwd(&outside, &root),
    )
    .unwrap_err();
    assert!(matches!(err, Error::HostError(message) if message.contains("escapes root")));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn proc_result_encodes_constructor_form() {
    let result = ProcResult {
        stdout: "out".to_owned(),
        stderr: "err".to_owned(),
        exit_code: 7,
        truncated: true,
    };

    let Expr::Call { operator, args } = result.to_constructor_expr() else {
        panic!("expected constructor call");
    };
    assert_eq!(*operator, Expr::Symbol(proc_result_symbol()));
    assert_eq!(args[0], Expr::String("out".to_owned()));
    assert_eq!(args[1], Expr::String("err".to_owned()));
    assert_eq!(args[3], Expr::Bool(true));
    assert_eq!(proc_result_symbol(), Symbol::new("ProcResult"));
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "sim-lib-exec-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(unix)]
fn wait_for_pid(path: &PathBuf) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Ok(contents) = fs::read_to_string(path) {
            return contents.trim().parse().unwrap();
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for pid file {}", path.display());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("env")
        .args([
            "sh",
            "-c",
            "kill -0 \"$1\" >/dev/null 2>&1",
            "sh",
            &pid.to_string(),
        ])
        .status()
        .unwrap()
        .success()
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    let _ = Command::new("env")
        .args([
            "sh",
            "-c",
            "kill -KILL \"$1\" >/dev/null 2>&1 || true",
            "sh",
            &pid.to_string(),
        ])
        .status();
}
