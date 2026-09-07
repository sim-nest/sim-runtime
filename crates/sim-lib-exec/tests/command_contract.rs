//! conformance: exact local-check commands bind all authority and refuse drift.

use sim_kernel::Datum;
use sim_lib_exec::{
    ArgAtom, CleanupContract, CommandInvocation, CommandReplayPolicy, CommandResource,
    CommandRoute, CommandSpec, MountAccess, NetworkAccess, OutputContract, OutputExpectation,
    OutputState, ProcessBudget, ProgramRef, ProjectRootRef, ResourceAccess, SandboxControl,
    SandboxLimits, SandboxMount, SandboxPolicy, SandboxRequirement, SealedBindings,
};

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

fn command(script: &[u8]) -> sim_kernel::Result<CommandSpec> {
    let resources = vec![
        CommandResource {
            source: "checkout".into(),
            guest_path: "/work".into(),
            access: ResourceAccess::Writable,
        },
        CommandResource {
            source: "scratch".into(),
            guest_path: "/scratch".into(),
            access: ResourceAccess::Writable,
        },
    ];
    let limits = SandboxLimits {
        cpu_seconds: 10,
        memory_bytes: 256 * 1024 * 1024,
        wall_time_ms: 5_000,
        process_count: 16,
        file_count: 1_000,
        file_bytes: 16 * 1024 * 1024,
        output_bytes: 16 * 1024,
        stdin_bytes: 1,
    };
    CommandSpec::new(
        ProgramRef::new("owner-shell")?,
        ProjectRootRef::new("checkout")?,
        CommandInvocation::Interpreter {
            flags: vec![ArgAtom::new("-c")?],
            script: script.to_vec(),
        },
        SealedBindings::literals([("PATH".into(), "/toolchain/bin".into())])?,
        resources.clone(),
        ProcessBudget {
            timeout_ms: limits.wall_time_ms,
            max_output_bytes: limits.output_bytes,
            stdin: None,
        },
        OutputContract::new(
            [0],
            vec![OutputExpectation {
                resource: "checkout".into(),
                relative_path: "target/check.stamp".into(),
                state: OutputState::FileContent(Datum::Bytes(b"pass\n".to_vec()).content_id()?),
            }],
        )?,
        CleanupContract::process_group(["scratch".into()])?,
        NetworkAccess::Absent,
        CommandRoute::Sandbox {
            launcher: "platform/sandbox/ubuntu-bwrap".into(),
            policy: SandboxPolicy::new(
                controls(),
                resources
                    .iter()
                    .map(|resource| SandboxMount {
                        source: resource.source.clone(),
                        guest_path: resource.guest_path.clone(),
                        access: match resource.access {
                            ResourceAccess::ReadOnly => MountAccess::ReadOnly,
                            ResourceAccess::Writable => MountAccess::Writable,
                        },
                    })
                    .collect(),
                limits,
            )?,
        },
        CommandReplayPolicy::ExactlyOnce,
    )
}

#[test]
fn public_contract_binds_exact_script_and_refuses_authority_drift() {
    let exact = command(b"cargo test --workspace").unwrap();
    assert_eq!(exact, command(b"cargo test --workspace").unwrap());
    assert_ne!(exact.id(), command(b"cargo test -p one").unwrap().id());
    assert!(matches!(exact.network(), NetworkAccess::Absent));
    assert_eq!(exact.cleanup().scratch_resources().len(), 1);

    let host_without_network_authority = CommandSpec::new(
        ProgramRef::new("owner-tool").unwrap(),
        ProjectRootRef::new("checkout").unwrap(),
        CommandInvocation::Argv(vec![]),
        SealedBindings::empty(),
        vec![CommandResource {
            source: "checkout".into(),
            guest_path: "/work".into(),
            access: ResourceAccess::Writable,
        }],
        ProcessBudget {
            timeout_ms: 1,
            max_output_bytes: 1,
            stdin: None,
        },
        OutputContract::new([0], vec![]).unwrap(),
        CleanupContract::process_group(Vec::new()).unwrap(),
        NetworkAccess::Absent,
        CommandRoute::Process,
        CommandReplayPolicy::Idempotent,
    );
    assert!(host_without_network_authority.is_err());
}
