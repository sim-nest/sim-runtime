//! Unit checks for exact command construction.

use crate::command::*;
use crate::{
    ArgAtom, MountAccess, ProcessBudget, ProgramRef, ProjectRootRef, SandboxControl, SandboxLimits,
    SandboxMount, SandboxPolicy, SandboxRequirement, SealedBindings,
};
use sim_kernel::CapabilityName;

fn controls() -> impl Iterator<Item = (SandboxControl, SandboxRequirement)> {
    [
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
    ]
    .into_iter()
    .map(|control| (control, SandboxRequirement::Required))
}
fn budget() -> ProcessBudget {
    ProcessBudget {
        timeout_ms: 1000,
        max_output_bytes: 4096,
        stdin: None,
    }
}
fn output() -> OutputContract {
    OutputContract::new(
        [0],
        vec![OutputExpectation {
            resource: "out".into(),
            relative_path: "result".into(),
            state: OutputState::Exists,
        }],
    )
    .unwrap()
}
fn cleanup() -> CleanupContract {
    CleanupContract::process_group(["out".into()]).unwrap()
}

#[test]
fn command_id_binds_script_bytes_environment_resources_and_bounds() {
    let create = |script: &[u8]| {
        CommandSpec::new(
            ProgramRef::new("shell").unwrap(),
            ProjectRootRef::new("checkout").unwrap(),
            CommandInvocation::Interpreter {
                flags: vec![ArgAtom::new("-c").unwrap()],
                script: script.to_vec(),
            },
            SealedBindings::literals([("PATH".into(), "/toolchain/bin".into())]).unwrap(),
            vec![CommandResource {
                source: "out".into(),
                guest_path: "/out".into(),
                access: ResourceAccess::Writable,
            }],
            budget(),
            output(),
            cleanup(),
            NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
            CommandRoute::Process,
            CommandReplayPolicy::Idempotent,
        )
        .unwrap()
    };
    let a = create(b"cargo test --workspace");
    let b = create(b"cargo test --workspace");
    let c = create(b"cargo test -p one");
    assert_eq!(a.id(), b.id());
    assert_ne!(a.id(), c.id());
    assert_eq!(
        a.invocation().argv().unwrap().last().unwrap().as_str(),
        "cargo test --workspace"
    );
}

#[test]
fn sandbox_route_requires_exact_mounts_and_absent_network() {
    let policy = SandboxPolicy::new(
        controls(),
        vec![SandboxMount {
            source: "out".into(),
            guest_path: "/out".into(),
            access: MountAccess::Writable,
        }],
        SandboxLimits {
            cpu_seconds: 1,
            memory_bytes: 1024,
            wall_time_ms: 1000,
            process_count: 4,
            file_count: 8,
            file_bytes: 4096,
            output_bytes: 4096,
            stdin_bytes: 1,
        },
    )
    .unwrap();
    let result = CommandSpec::new(
        ProgramRef::new("tool").unwrap(),
        ProjectRootRef::new("checkout").unwrap(),
        CommandInvocation::Argv(vec![]),
        SealedBindings::empty(),
        vec![],
        budget(),
        output(),
        cleanup(),
        NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
        CommandRoute::Sandbox {
            launcher: "bwrap".into(),
            policy,
        },
        CommandReplayPolicy::ExactlyOnce,
    );
    assert!(result.is_err(), "undeclared sandbox mount must fail closed");
}

#[test]
fn output_paths_and_cleanup_cannot_escape_writable_resources() {
    assert!(
        OutputContract::new(
            [0],
            vec![OutputExpectation {
                resource: "out".into(),
                relative_path: "../escape".into(),
                state: OutputState::Exists
            }]
        )
        .is_err()
    );
    let result = CommandSpec::new(
        ProgramRef::new("tool").unwrap(),
        ProjectRootRef::new("checkout").unwrap(),
        CommandInvocation::Argv(vec![]),
        SealedBindings::empty(),
        vec![],
        budget(),
        output(),
        cleanup(),
        NetworkAccess::Absent,
        CommandRoute::Process,
        CommandReplayPolicy::ExactlyOnce,
    );
    assert!(result.is_err());
}
