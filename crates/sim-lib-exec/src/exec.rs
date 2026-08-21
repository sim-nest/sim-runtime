use sim_kernel::{CapabilityName, Cx, Error, Expr, NumberLiteral, Result, Symbol};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Capability required before a process request reaches its port.
pub fn exec_capability() -> CapabilityName {
    CapabilityName::new("exec")
}
/// Read-constructor symbol for process results.
pub fn proc_result_symbol() -> Symbol {
    Symbol::new("ProcResult")
}

/// Bounded process policy, including an explicit confinement root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOptions {
    /// Requested working directory.
    pub cwd: PathBuf,
    /// Explicit confinement root.
    pub root: PathBuf,
    /// Required timeout in milliseconds.
    pub timeout_ms: u64,
    /// Shared stdout and stderr byte budget.
    pub max_output_bytes: usize,
    /// Optional standard input.
    pub stdin: Option<Vec<u8>>,
    /// Exact child environment; inherited variables are excluded.
    pub environment: BTreeMap<String, String>,
}
impl ExecOptions {
    /// Creates bounded options rooted at `root`.
    /// Selects a working directory, still confined by the existing root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, timeout_ms: u64, max_output_bytes: usize) -> Self {
        let root = root.into();
        Self {
            cwd: root.clone(),
            root,
            timeout_ms,
            max_output_bytes,
            stdin: None,
            environment: BTreeMap::new(),
        }
    }
    /// Selects a working directory, still confined by the existing root.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }
    /// Supplies bounded standard input.
    #[must_use]
    pub fn with_stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }
    /// Admits one environment entry.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }
}

/// Fully checked request passed to a platform adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRequest {
    /// Program followed by verbatim arguments.
    pub argv: Vec<String>,
    /// Checked, explicit working directory.
    pub cwd: PathBuf,
    /// Checked, explicit confinement root.
    pub root: PathBuf,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Shared output budget.
    pub max_output_bytes: usize,
    /// Optional standard input.
    pub stdin: Option<Vec<u8>>,
    /// Diminished child environment.
    pub environment: BTreeMap<String, String>,
}

/// Cooperative cancellation observed by platform process adapters.
#[derive(Clone, Debug, Default)]
pub struct ProcessCancellation(Arc<AtomicBool>);
impl ProcessCancellation {
    /// Requests process-tree cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Stable bounded process result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcResult {
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Exit status, or -1 when unavailable.
    pub exit_code: i32,
    /// Whether the shared output budget truncated either stream.
    pub truncated: bool,
}
impl ProcResult {
    /// Converts the result to its stable read-constructor expression.
    #[must_use]
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

/// Platform-mechanics failures with cleanup evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    /// Spawn failed.
    Spawn(String),
    /// Pipe, wait, or other process IO failed.
    Io(String),
    /// The adapter could not prove cwd confinement.
    Confinement(String),
    /// The monotonic deadline expired.
    Timeout {
        /// Cleanup failure, when any.
        kill_failure: Option<String>,
        /// Whether descendants remained after cleanup.
        leaked_descendants: bool,
    },
    /// Cancellation won the completion race.
    Cancelled {
        /// Cleanup failure, when any.
        kill_failure: Option<String>,
        /// Whether descendants remained after cleanup.
        leaked_descendants: bool,
    },
}
/// Platform execution receipt using only privacy-safe evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessReceipt {
    /// Stable platform provider id.
    pub provider: String,
    /// Elapsed monotonic time reported by the adapter.
    pub elapsed_mono_ns: u64,
    /// Bounded result.
    pub result: ProcResult,
}

/// Runtime-owned seam implemented by model and physical platform capsules.
pub trait ProcessPort: Send + Sync {
    /// Realizes an already checked request and owns all cleanup mechanics.
    fn run(
        &self,
        request: &ProcessRequest,
        cancellation: &ProcessCancellation,
    ) -> std::result::Result<ProcessReceipt, ProcessError>;
}

/// Checks capability and policy before dispatching mechanics to `port`.
pub fn exec(
    cx: &mut Cx,
    port: &dyn ProcessPort,
    argv: &[String],
    options: &ExecOptions,
    cancellation: &ProcessCancellation,
) -> Result<ProcResult> {
    cx.require(&exec_capability())?;
    let request = checked_request(argv, options)?;
    port.run(&request, cancellation)
        .map(|receipt| receipt.result)
        .map_err(map_process_error)
}

fn checked_request(argv: &[String], options: &ExecOptions) -> Result<ProcessRequest> {
    if argv.is_empty() || argv.iter().any(String::is_empty) {
        return Err(Error::Eval("exec requires a non-empty argv vector".into()));
    }
    if options.timeout_ms == 0 {
        return Err(Error::Eval("exec requires a non-zero timeout_ms".into()));
    }
    if options.max_output_bytes == 0 {
        return Err(Error::Eval("exec requires a non-zero output budget".into()));
    }
    if !options.root.is_absolute() || !options.cwd.is_absolute() {
        return Err(Error::Eval("exec root and cwd must be absolute".into()));
    }
    if options
        .environment
        .keys()
        .any(|key| key.is_empty() || key.contains(['=', '\0']))
        || options
            .environment
            .values()
            .any(|value| value.contains('\0'))
    {
        return Err(Error::Eval(
            "exec environment contains an invalid entry".into(),
        ));
    }
    Ok(ProcessRequest {
        argv: argv.to_vec(),
        cwd: options.cwd.clone(),
        root: options.root.clone(),
        timeout_ms: options.timeout_ms,
        max_output_bytes: options.max_output_bytes,
        stdin: options.stdin.clone(),
        environment: options.environment.clone(),
    })
}

fn map_process_error(error: ProcessError) -> Error {
    let detail = match error {
        ProcessError::Spawn(v) => format!("exec spawn: {v}"),
        ProcessError::Io(v) => format!("exec host io: {v}"),
        ProcessError::Confinement(v) => format!("exec confinement: {v}"),
        ProcessError::Timeout {
            kill_failure,
            leaked_descendants,
        } => terminal_error("timed out", kill_failure, leaked_descendants),
        ProcessError::Cancelled {
            kill_failure,
            leaked_descendants,
        } => terminal_error("cancelled", kill_failure, leaked_descendants),
    };
    Error::HostError(detail)
}
fn terminal_error(kind: &str, kill_failure: Option<String>, leaked: bool) -> String {
    let mut detail = format!("exec {kind}");
    if let Some(v) = kill_failure {
        detail.push_str(&format!("; kill failed: {v}"));
    }
    if leaked {
        detail.push_str("; leaked descendant detected");
    }
    detail
}
