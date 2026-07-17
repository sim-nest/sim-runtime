use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use sim_kernel::{CapabilityName, Cx, Error, Expr, NumberLiteral, Result, Symbol};

/// Returns the capability required by [`exec`].
pub fn exec_capability() -> CapabilityName {
    CapabilityName::new("exec")
}

/// Returns the constructor symbol used by [`ProcResult::to_constructor_expr`].
pub fn proc_result_symbol() -> Symbol {
    Symbol::new("ProcResult")
}

/// Bounded process execution options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOptions {
    /// Directory the child runs in.
    ///
    /// When set, the directory must canonicalize inside [`root`](Self::root).
    pub cwd: Option<PathBuf>,
    /// Root that confines [`cwd`](Self::cwd).
    ///
    /// When `cwd` is set and this is absent, the current process directory is
    /// the confinement root. When this is set and `cwd` is absent, the child
    /// runs in this root directory.
    pub root: Option<PathBuf>,
    /// Mandatory timeout in milliseconds. Zero is rejected before spawning.
    pub timeout_ms: u64,
    /// Maximum total captured stdout plus stderr bytes.
    pub max_output_bytes: usize,
    /// Optional stdin bytes written to the child.
    pub stdin: Option<Vec<u8>>,
}

impl ExecOptions {
    /// Builds process options with a timeout and output cap.
    pub fn new(timeout_ms: u64, max_output_bytes: usize) -> Self {
        Self {
            cwd: None,
            root: None,
            timeout_ms,
            max_output_bytes,
            stdin: None,
        }
    }

    /// Returns options with a confined working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self.root = Some(root.into());
        self
    }

    /// Returns options with stdin bytes.
    pub fn with_stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }
}

/// Result of a bounded process run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcResult {
    /// Captured stdout, lossily decoded as UTF-8 after byte capping.
    pub stdout: String,
    /// Captured stderr, lossily decoded as UTF-8 after byte capping.
    pub stderr: String,
    /// Process exit code, or `-1` when the host reports no numeric code.
    pub exit_code: i32,
    /// Whether stdout or stderr exceeded the shared output byte cap.
    pub truncated: bool,
}

impl ProcResult {
    /// Encodes this result as `#(ProcResult stdout stderr exit_code truncated)`.
    pub fn to_constructor_expr(&self) -> Expr {
        Expr::Call {
            operator: Box::new(Expr::Symbol(proc_result_symbol())),
            args: vec![
                Expr::String(self.stdout.clone()),
                Expr::String(self.stderr.clone()),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "i64"),
                    canonical: self.exit_code.to_string(),
                }),
                Expr::Bool(self.truncated),
            ],
        }
    }
}

/// Runs one host process with explicit argv and bounded output.
///
/// The caller must hold [`exec_capability`]. The argv list must be non-empty;
/// the first element is the program and the remaining elements are passed
/// verbatim as arguments. No shell is inserted by this function.
pub fn exec(cx: &mut Cx, argv: &[String], options: &ExecOptions) -> Result<ProcResult> {
    cx.require(&exec_capability())?;
    validate_request(argv, options)?;

    #[cfg(not(unix))]
    {
        return Err(Error::HostError(
            "exec is unavailable on this platform until process-tree timeout enforcement is implemented"
                .to_owned(),
        ));
    }

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    if let Some(cwd) = confined_cwd(options)? {
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .map_err(|err| Error::HostError(format!("exec spawn {}: {err}", argv[0])))?;
    run_child(&mut child, options)
}

fn validate_request(argv: &[String], options: &ExecOptions) -> Result<()> {
    if argv.is_empty() {
        return Err(Error::Eval(
            "exec requires a non-empty argv list".to_owned(),
        ));
    }
    if options.timeout_ms == 0 {
        return Err(Error::Eval(
            "exec requires a non-zero timeout_ms".to_owned(),
        ));
    }
    Ok(())
}

fn confined_cwd(options: &ExecOptions) -> Result<Option<PathBuf>> {
    if options.cwd.is_none() && options.root.is_none() {
        return Ok(None);
    }

    let root = match &options.root {
        Some(root) => root.clone(),
        None => std::env::current_dir()
            .map_err(|err| Error::HostError(format!("exec current dir: {err}")))?,
    };
    let cwd = options.cwd.clone().unwrap_or_else(|| root.clone());
    let root = canonicalize_path(root, "exec root")?;
    let cwd = canonicalize_path(cwd, "exec cwd")?;
    if !cwd.starts_with(&root) {
        return Err(Error::HostError(format!(
            "exec cwd {} escapes root {}",
            cwd.display(),
            root.display()
        )));
    }
    Ok(Some(cwd))
}

fn canonicalize_path(path: PathBuf, label: &'static str) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|err| Error::HostError(format!("{label} {}: {err}", path.display())))
}

fn run_child(child: &mut Child, options: &ExecOptions) -> Result<ProcResult> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::HostError("exec stdout pipe missing".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::HostError("exec stderr pipe missing".to_owned()))?;
    let stdin = child.stdin.take();

    let budget = Arc::new(Mutex::new(CaptureBudget::new(options.max_output_bytes)));
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(options.timeout_ms))
        .ok_or_else(|| Error::Eval("exec timeout is too large".to_owned()))?;

    let (tx, rx) = mpsc::channel();
    spawn_reader(
        stdout,
        Arc::clone(&budget),
        CaptureStream::Stdout,
        tx.clone(),
    );
    spawn_reader(
        stderr,
        Arc::clone(&budget),
        CaptureStream::Stderr,
        tx.clone(),
    );
    let stdin_pending = if let Some(stdin) = stdin {
        spawn_writer(stdin, options.stdin.clone(), tx.clone());
        true
    } else {
        false
    };
    drop(tx);

    let completion = wait_for_completion(child, &rx, deadline, stdin_pending);
    match completion {
        Ok(ChildCompletion {
            status,
            stdout,
            stderr,
            stdin,
        }) => {
            stdin?;
            let stdout = stdout?;
            let stderr = stderr?;
            let truncated = budget
                .lock()
                .map_err(|_| Error::PoisonedLock("exec output budget"))?
                .truncated;
            Ok(ProcResult {
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
                exit_code: exit_code(status),
                truncated,
            })
        }
        Err(WaitError::Timeout { child_exited }) => {
            Err(timeout_error(child, options.timeout_ms, child_exited))
        }
        Err(WaitError::Host(err)) => Err(err),
    }
}

fn spawn_reader<R>(
    reader: R,
    budget: Arc<Mutex<CaptureBudget>>,
    stream: CaptureStream,
    tx: Sender<ChildEvent>,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_capped(reader, budget, stream.name())
        }))
        .unwrap_or_else(|_| {
            Err(Error::HostError(format!(
                "exec {} thread panicked",
                stream.name()
            )))
        });
        let _ = tx.send(ChildEvent::Capture { stream, result });
    });
}

fn spawn_writer(
    mut stdin: std::process::ChildStdin,
    input: Option<Vec<u8>>,
    tx: Sender<ChildEvent>,
) {
    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write_stdin(&mut stdin, input)
        }))
        .unwrap_or_else(|_| Err(Error::HostError("exec stdin thread panicked".to_owned())));
        let _ = tx.send(ChildEvent::Stdin(result));
    });
}

fn write_stdin(stdin: &mut std::process::ChildStdin, input: Option<Vec<u8>>) -> Result<()> {
    let Some(input) = input else {
        return Ok(());
    };
    match stdin.write_all(&input) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(Error::HostError(format!("exec stdin write: {err}"))),
    }
}

fn wait_for_completion(
    child: &mut Child,
    rx: &Receiver<ChildEvent>,
    deadline: Instant,
    stdin_pending: bool,
) -> std::result::Result<ChildCompletion, WaitError> {
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;
    let mut stdin = if stdin_pending { None } else { Some(Ok(())) };

    loop {
        poll_child_status(child, &mut status)?;
        drain_child_events(rx, &mut stdout, &mut stderr, &mut stdin)?;
        if status.is_some() && stdout.is_some() && stderr.is_some() && stdin.is_some() {
            return Ok(ChildCompletion {
                status: status.take().expect("status checked above"),
                stdout: stdout.take().expect("stdout checked above"),
                stderr: stderr.take().expect("stderr checked above"),
                stdin: stdin.take().expect("stdin checked above"),
            });
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(WaitError::Timeout {
                child_exited: status.is_some(),
            });
        }

        match rx.recv_timeout((deadline - now).min(Duration::from_millis(10))) {
            Ok(event) => record_child_event(event, &mut stdout, &mut stderr, &mut stdin),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) if status.is_some() => {
                return Err(WaitError::Host(Error::HostError(
                    "exec capture thread ended without result".to_owned(),
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                thread::sleep((deadline - now).min(Duration::from_millis(10)));
            }
        }
    }
}

fn poll_child_status(
    child: &mut Child,
    status: &mut Option<ExitStatus>,
) -> std::result::Result<(), WaitError> {
    if status.is_some() {
        return Ok(());
    }
    *status = child
        .try_wait()
        .map_err(|err| WaitError::Host(Error::HostError(format!("exec wait: {err}"))))?;
    Ok(())
}

fn drain_child_events(
    rx: &Receiver<ChildEvent>,
    stdout: &mut Option<Result<Vec<u8>>>,
    stderr: &mut Option<Result<Vec<u8>>>,
    stdin: &mut Option<Result<()>>,
) -> std::result::Result<(), WaitError> {
    loop {
        match rx.try_recv() {
            Ok(event) => record_child_event(event, stdout, stderr, stdin),
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

fn record_child_event(
    event: ChildEvent,
    stdout: &mut Option<Result<Vec<u8>>>,
    stderr: &mut Option<Result<Vec<u8>>>,
    stdin: &mut Option<Result<()>>,
) {
    match event {
        ChildEvent::Capture {
            stream: CaptureStream::Stdout,
            result,
        } => *stdout = Some(result),
        ChildEvent::Capture {
            stream: CaptureStream::Stderr,
            result,
        } => *stderr = Some(result),
        ChildEvent::Stdin(result) => *stdin = Some(result),
    }
}

fn timeout_error(child: &mut Child, timeout_ms: u64, child_exited: bool) -> Error {
    let kill_result = if child_exited {
        Ok(())
    } else {
        terminate_timed_out_child(child)
    };
    let wait_result = child.wait();
    let mut message = format!("exec timed out after {timeout_ms} ms");
    if let Err(err) = kill_result {
        message.push_str(&format!("; kill failed: {err}"));
    }
    if let Err(err) = wait_result {
        message.push_str(&format!("; wait failed: {err}"));
    }
    Error::HostError(message)
}

#[cfg(unix)]
fn terminate_timed_out_child(child: &mut Child) -> io::Result<()> {
    let pgid = child.id();
    if pgid == 0 {
        return child.kill();
    }

    send_process_group_signal(pgid, "TERM")?;
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    send_process_group_signal(pgid, "KILL")
}

#[cfg(not(unix))]
fn terminate_timed_out_child(child: &mut Child) -> io::Result<()> {
    child.kill()
}

#[cfg(unix)]
fn send_process_group_signal(pgid: u32, signal: &str) -> io::Result<()> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pgid}"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "kill {signal} process group {pgid} exited with {status}"
        )))
    }
}

struct ChildCompletion {
    status: ExitStatus,
    stdout: Result<Vec<u8>>,
    stderr: Result<Vec<u8>>,
    stdin: Result<()>,
}

enum WaitError {
    Timeout { child_exited: bool },
    Host(Error),
}

#[derive(Clone, Copy)]
enum CaptureStream {
    Stdout,
    Stderr,
}

impl CaptureStream {
    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

enum ChildEvent {
    Capture {
        stream: CaptureStream,
        result: Result<Vec<u8>>,
    },
    Stdin(Result<()>),
}

fn read_capped<R>(
    mut reader: R,
    budget: Arc<Mutex<CaptureBudget>>,
    name: &'static str,
) -> Result<Vec<u8>>
where
    R: Read,
{
    let mut captured = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|err| Error::HostError(format!("exec read {name}: {err}")))?;
        if read == 0 {
            return Ok(captured);
        }
        let keep = {
            let mut budget = budget
                .lock()
                .map_err(|_| Error::PoisonedLock("exec output budget"))?;
            budget.claim(read)
        };
        captured.extend_from_slice(&chunk[..keep]);
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

#[derive(Debug)]
struct CaptureBudget {
    remaining: usize,
    truncated: bool,
}

impl CaptureBudget {
    fn new(max_output_bytes: usize) -> Self {
        Self {
            remaining: max_output_bytes,
            truncated: false,
        }
    }

    fn claim(&mut self, read: usize) -> usize {
        let keep = read.min(self.remaining);
        self.remaining -= keep;
        if keep < read {
            self.truncated = true;
        }
        keep
    }
}
