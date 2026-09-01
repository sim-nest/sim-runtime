use sim_kernel::{CapabilityName, Cx, Error, Expr, NumberLiteral, Result, Symbol};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_BINDINGS: usize = 128;
const MAX_BINDING_BYTES: usize = 64 * 1024;
/// Capability required before a process request reaches its port.
pub fn exec_capability() -> CapabilityName {
    CapabilityName::new("exec")
}
/// Read-constructor symbol for process results.
pub fn proc_result_symbol() -> Symbol {
    Symbol::new("ProcResult")
}

macro_rules! opaque_ref {
    ($name:ident, $label:literal) => {
        #[doc = concat!("Opaque, boot-trusted ", $label, ".")]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            /// Validates and creates an opaque reference.
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.is_empty() || value.contains('\0') {
                    return Err(Error::Eval(
                        concat!($label, " must be non-empty and NUL-free").into(),
                    ));
                }
                Ok(Self(value))
            }
            #[must_use]
            /// Returns the non-native reference identifier.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
opaque_ref!(ProgramRef, "program reference");
opaque_ref!(ProjectRootRef, "project-root reference");
opaque_ref!(PrivateArtifactRef, "private-artifact reference");

/// One whole, NUL-free native argument; it is never shell-split.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgAtom(String);
impl ArgAtom {
    /// Validates and creates one whole argument.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.contains('\0') {
            return Err(Error::Eval("argument contains NUL".into()));
        }
        Ok(Self(value))
    }
    #[must_use]
    /// Returns the literal argument.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A sealed literal or capsule-rendered resource reference.
pub enum BindingValue {
    /// Exact literal value.
    Literal(String),
    /// Opaque project root rendered by the capsule.
    ProjectRoot(ProjectRootRef),
    /// Opaque private artifact rendered by the capsule.
    PrivateArtifact(PrivateArtifactRef),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Exact child bindings. No ambient inheritance is representable.
pub struct SealedBindings(BTreeMap<String, BindingValue>);
impl SealedBindings {
    /// Creates the secure empty default.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
    /// Validates names, values, duplicates, count, and total bytes.
    pub fn try_from_entries(
        entries: impl IntoIterator<Item = (String, BindingValue)>,
    ) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut bytes = 0usize;
        for (name, value) in entries {
            if name.is_empty() || name.contains(['=', '\0']) {
                return Err(Error::Eval("sealed binding has an invalid name".into()));
            }
            let value_bytes = match &value {
                BindingValue::Literal(v) => {
                    if v.contains('\0') {
                        return Err(Error::Eval("sealed binding literal contains NUL".into()));
                    }
                    v.len()
                }
                BindingValue::ProjectRoot(v) => v.as_str().len(),
                BindingValue::PrivateArtifact(v) => v.as_str().len(),
            };
            bytes = bytes.saturating_add(name.len()).saturating_add(value_bytes);
            if values.insert(name, value).is_some() {
                return Err(Error::Eval("duplicate sealed binding".into()));
            }
            if values.len() > MAX_BINDINGS || bytes > MAX_BINDING_BYTES {
                return Err(Error::Eval("sealed bindings exceed bounded size".into()));
            }
        }
        Ok(Self(values))
    }
    /// Creates explicit literal bindings, for boot-supplied compatibility data.
    pub fn literals(entries: impl IntoIterator<Item = (String, String)>) -> Result<Self> {
        Self::try_from_entries(
            entries
                .into_iter()
                .map(|(k, v)| (k, BindingValue::Literal(v))),
        )
    }
    /// Iterates exact bindings for capsule rendering.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &BindingValue)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Bounded input, time, and output policy.
pub struct ProcessBudget {
    /// Required timeout.
    pub timeout_ms: u64,
    /// Shared stdout/stderr byte cap.
    pub max_output_bytes: usize,
    /// Optional standard input.
    pub stdin: Option<Vec<u8>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Portable options used to create a sealed process request.
pub struct ExecOptions {
    /// Boot-trusted program identity.
    pub program: ProgramRef,
    /// Opaque working project identity.
    pub root: ProjectRootRef,
    /// Resource budget.
    pub budget: ProcessBudget,
    /// Exact, empty-by-default child environment.
    pub environment: SealedBindings,
    /// Declared private artifacts available to bindings.
    pub private_artifacts: Vec<PrivateArtifactRef>,
}
impl ExecOptions {
    /// Creates options with an empty sealed environment.
    pub fn new(
        program: ProgramRef,
        root: ProjectRootRef,
        timeout_ms: u64,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            program,
            root,
            budget: ProcessBudget {
                timeout_ms,
                max_output_bytes,
                stdin: None,
            },
            environment: SealedBindings::empty(),
            private_artifacts: Vec::new(),
        }
    }
    #[must_use]
    /// Supplies bounded standard input.
    pub fn with_stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.budget.stdin = Some(stdin.into());
        self
    }
    #[must_use]
    /// Supplies explicitly validated bindings.
    pub fn with_bindings(mut self, bindings: SealedBindings) -> Self {
        self.environment = bindings;
        self
    }
    #[must_use]
    /// Declares private artifacts that the capsule may render.
    pub fn with_private_artifacts(mut self, artifacts: Vec<PrivateArtifactRef>) -> Self {
        self.private_artifacts = artifacts;
        self
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Fully validated portable request passed to a platform capsule.
pub struct ProcessRequest {
    /// Boot-trusted program identity.
    pub program: ProgramRef,
    /// Whole literal arguments.
    pub argv: Vec<ArgAtom>,
    /// Opaque project root.
    pub root: ProjectRootRef,
    /// Exact sealed child environment.
    pub environment: SealedBindings,
    /// Declared private resources.
    pub private_artifacts: Vec<PrivateArtifactRef>,
    /// Bounded execution budget.
    pub budget: ProcessBudget,
}

#[derive(Clone, Debug, Default)]
/// Cooperative cancellation token shared with the platform adapter.
pub struct ProcessCancellation(Arc<AtomicBool>);
impl ProcessCancellation {
    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release)
    }
    #[must_use]
    /// Reports whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Stable bounded process result; non-zero exit remains a result.
pub struct ProcResult {
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Native exit code, or -1 when unavailable.
    pub exit_code: i32,
    /// Whether output exceeded its shared cap.
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
#[derive(Clone, Debug, PartialEq, Eq)]
/// Privacy-safe completed-process receipt.
pub struct ProcessReceipt {
    /// Stable capsule identity.
    pub provider: String,
    /// Elapsed monotonic time.
    pub elapsed_mono_ns: u64,
    /// Completed result.
    pub result: ProcResult,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Proof that a dispatched process group was killed and reaped.
pub struct StopReceipt {
    /// Stable capsule identity.
    pub provider: String,
    /// Elapsed monotonic time.
    pub elapsed_mono_ns: u64,
    /// Bounded cleanup evidence.
    pub cleanup: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Bounded evidence for an ambiguous post-spawn outcome.
pub struct DispatchEvidence {
    /// Stable capsule identity.
    pub provider: String,
    /// Failed post-spawn stage.
    pub stage: String,
    /// Sanitized bounded detail.
    pub detail: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Reason a process definitely did not cross the spawn boundary.
pub enum ProcessRefusal {
    /// Portable request validation failed.
    Invalid(String),
    /// Capsule policy or resource resolution refused the request.
    Refused(String),
    /// Native spawn failed before dispatch.
    SpawnFailed(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Exact dispatch truth for one process attempt.
pub enum ProcessAttempt {
    /// Spawn definitely did not succeed.
    NotDispatched {
        /// Pre-spawn refusal evidence.
        refusal: ProcessRefusal,
    },
    /// The child completed, including non-zero exit.
    Completed {
        /// Completion receipt.
        receipt: ProcessReceipt,
    },
    /// Timeout won and cleanup was proven.
    StoppedAfterTimeout {
        /// Proven cleanup receipt.
        receipt: StopReceipt,
    },
    /// Cancellation won and cleanup was proven.
    StoppedAfterCancel {
        /// Proven cleanup receipt.
        receipt: StopReceipt,
    },
    /// Spawn succeeded but final state is ambiguous.
    UnknownAfterDispatch {
        /// Bounded ambiguity evidence.
        evidence: DispatchEvidence,
    },
}
impl ProcessAttempt {
    /// Returns true only when automatic retry cannot duplicate dispatched work.
    #[must_use]
    pub fn automatically_retryable(&self) -> bool {
        matches!(self, Self::NotDispatched { .. })
    }
}
/// Runtime-owned seam implemented only by model and physical capsules.
pub trait ProcessPort: Send + Sync {
    /// Resolves opaque resources and executes one sealed request.
    fn run(&self, request: &ProcessRequest, cancellation: &ProcessCancellation) -> ProcessAttempt;
}

/// Checks capability and portable policy before invoking the port.
pub fn exec(
    cx: &mut Cx,
    port: &dyn ProcessPort,
    argv: &[String],
    options: &ExecOptions,
    cancellation: &ProcessCancellation,
) -> Result<ProcResult> {
    cx.require(&exec_capability())?;
    let request = checked_request(argv, options)?;
    match port.run(&request, cancellation) {
        ProcessAttempt::Completed { receipt } => Ok(receipt.result),
        attempt => Err(Error::HostError(format!("exec attempt: {attempt:?}"))),
    }
}
fn checked_request(argv: &[String], options: &ExecOptions) -> Result<ProcessRequest> {
    if options.budget.timeout_ms == 0 {
        return Err(Error::Eval("exec requires a non-zero timeout_ms".into()));
    }
    if options.budget.max_output_bytes == 0 {
        return Err(Error::Eval("exec requires a non-zero output budget".into()));
    }
    let argv = argv
        .iter()
        .cloned()
        .map(ArgAtom::new)
        .collect::<Result<Vec<_>>>()?;
    Ok(ProcessRequest {
        program: options.program.clone(),
        argv,
        root: options.root.clone(),
        environment: options.environment.clone(),
        private_artifacts: options.private_artifacts.clone(),
        budget: options.budget.clone(),
    })
}
