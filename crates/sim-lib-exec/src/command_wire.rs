//! Canonical semantic projection for exact command contracts.

use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};

use crate::{
    BindingValue, ProcessBudget, SandboxControl, SandboxCpuLimit, SandboxFilesystemLimit,
    SandboxMemoryLimit, SandboxRequirement, SealedBindings,
    command::{
        CommandInvocation, CommandReplayPolicy, CommandResource, CommandRoute, NetworkAccess,
        OutputExpectation, OutputState, ResourceAccess,
    },
};

pub(crate) fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("local-check", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}
pub(crate) fn id_datum(id: &ContentId) -> Datum {
    node(
        "content-id-v1",
        vec![
            ("algorithm", Datum::Symbol(id.algorithm.clone())),
            ("digest", Datum::Bytes(id.bytes.to_vec())),
        ],
    )
}
pub(super) fn i64_datum(value: i64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}
pub(crate) fn u64_datum(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}
fn usize_datum(value: usize) -> Datum {
    u64_datum(u64::try_from(value).unwrap_or(u64::MAX))
}
pub(super) fn invocation_datum(invocation: &CommandInvocation) -> Datum {
    match invocation {
        CommandInvocation::Argv(argv) => node(
            "argv-v1",
            vec![(
                "argv",
                Datum::Vector(
                    argv.iter()
                        .map(|arg| Datum::String(arg.as_str().into()))
                        .collect(),
                ),
            )],
        ),
        CommandInvocation::Interpreter { flags, script } => node(
            "interpreter-v1",
            vec![
                (
                    "flags",
                    Datum::Vector(
                        flags
                            .iter()
                            .map(|arg| Datum::String(arg.as_str().into()))
                            .collect(),
                    ),
                ),
                ("script", Datum::Bytes(script.clone())),
            ],
        ),
    }
}
pub(super) fn environment_datum(environment: &SealedBindings) -> Datum {
    node(
        "environment-v1",
        vec![(
            "bindings",
            Datum::Map(
                environment
                    .iter()
                    .map(|(name, value)| {
                        (
                            Datum::String(name.into()),
                            match value {
                                BindingValue::Literal(value) => node(
                                    "literal-v1",
                                    vec![("value", Datum::String(value.clone()))],
                                ),
                                BindingValue::ProjectRoot(value) => node(
                                    "project-root-v1",
                                    vec![("value", Datum::String(value.as_str().into()))],
                                ),
                                BindingValue::PrivateArtifact(value) => node(
                                    "private-artifact-v1",
                                    vec![("value", Datum::String(value.as_str().into()))],
                                ),
                            },
                        )
                    })
                    .collect(),
            ),
        )],
    )
}
pub(super) fn resource_datum(resource: &CommandResource) -> Datum {
    node(
        "resource-v1",
        vec![
            ("source", Datum::String(resource.source.clone())),
            ("guest-path", Datum::String(resource.guest_path.clone())),
            (
                "access",
                Datum::Symbol(Symbol::qualified(
                    "resource-access",
                    match resource.access {
                        ResourceAccess::ReadOnly => "read-only",
                        ResourceAccess::Writable => "writable",
                    },
                )),
            ),
        ],
    )
}
pub(super) fn output_datum(output: &OutputExpectation) -> Datum {
    node(
        "output-v1",
        vec![
            ("resource", Datum::String(output.resource.clone())),
            ("relative-path", Datum::String(output.relative_path.clone())),
            (
                "state",
                match &output.state {
                    OutputState::Exists => Datum::Symbol(Symbol::qualified("output", "exists")),
                    OutputState::Absent => Datum::Symbol(Symbol::qualified("output", "absent")),
                    OutputState::FileContent(id) => id_datum(id),
                },
            ),
        ],
    )
}
pub(super) fn budget_datum(budget: &ProcessBudget) -> Datum {
    node(
        "budget-v1",
        vec![
            ("timeout-ms", u64_datum(budget.timeout_ms)),
            ("max-output-bytes", usize_datum(budget.max_output_bytes)),
            (
                "stdin",
                budget
                    .stdin
                    .as_ref()
                    .map_or(Datum::Nil, |value| Datum::Bytes(value.clone())),
            ),
        ],
    )
}
pub(super) fn network_datum(network: &NetworkAccess) -> Datum {
    match network {
        NetworkAccess::Absent => Datum::Symbol(Symbol::qualified("network", "absent")),
        NetworkAccess::Scoped(capability) => node(
            "network-scoped-v1",
            vec![("capability", Datum::String(capability.as_str().into()))],
        ),
    }
}
pub(super) fn route_datum(route: &CommandRoute) -> Datum {
    match route {
        CommandRoute::Process => Datum::Symbol(Symbol::qualified("command-route", "process")),
        CommandRoute::Sandbox { launcher, policy } => node(
            "sandbox-route-v2",
            vec![
                ("launcher", Datum::String(launcher.clone())),
                (
                    "requirements",
                    Datum::Map(
                        policy
                            .requirements()
                            .iter()
                            .map(|(control, requirement)| {
                                (
                                    Datum::Symbol(Symbol::qualified(
                                        "sandbox-control",
                                        control_name(*control),
                                    )),
                                    Datum::Symbol(Symbol::qualified(
                                        "sandbox-requirement",
                                        requirement_name(*requirement),
                                    )),
                                )
                            })
                            .collect(),
                    ),
                ),
                (
                    "mounts",
                    Datum::Vector(
                        policy
                            .mounts()
                            .iter()
                            .map(|mount| {
                                node(
                                    "mount-v1",
                                    vec![
                                        ("source", Datum::String(mount.source.clone())),
                                        ("guest-path", Datum::String(mount.guest_path.clone())),
                                        (
                                            "access",
                                            Datum::Symbol(Symbol::qualified(
                                                "mount-access",
                                                match mount.access {
                                                    crate::MountAccess::ReadOnly => "read-only",
                                                    crate::MountAccess::Writable => "writable",
                                                },
                                            )),
                                        ),
                                    ],
                                )
                            })
                            .collect(),
                    ),
                ),
                (
                    "limits",
                    node(
                        "limits-v2",
                        vec![
                            ("cpu", cpu_limit_datum(policy.limits().cpu)),
                            ("memory", memory_limit_datum(policy.limits().memory)),
                            ("wall-time-ms", u64_datum(policy.limits().wall_time_ms)),
                            ("process-count", u64_datum(policy.limits().process_count)),
                            (
                                "filesystem",
                                filesystem_limit_datum(policy.limits().filesystem),
                            ),
                            ("output-bytes", usize_datum(policy.limits().output_bytes)),
                            ("stdin-bytes", usize_datum(policy.limits().stdin_bytes)),
                        ],
                    ),
                ),
            ],
        ),
    }
}
fn cpu_limit_datum(limit: SandboxCpuLimit) -> Datum {
    match limit {
        SandboxCpuLimit::PerProcessSeconds(seconds) => node(
            "cpu-per-process-time-v1",
            vec![("seconds", u64_datum(seconds))],
        ),
        SandboxCpuLimit::Rate {
            quota_us,
            period_us,
        } => node(
            "cpu-aggregate-rate-v1",
            vec![
                ("quota-us", u64_datum(quota_us)),
                ("period-us", u64_datum(period_us)),
            ],
        ),
    }
}
fn memory_limit_datum(limit: SandboxMemoryLimit) -> Datum {
    match limit {
        SandboxMemoryLimit::PerProcessAddressSpaceBytes(bytes) => node(
            "memory-per-process-address-space-v1",
            vec![("bytes", u64_datum(bytes))],
        ),
        SandboxMemoryLimit::Charged { bytes, swap_bytes } => node(
            "memory-aggregate-charge-v1",
            vec![
                ("bytes", u64_datum(bytes)),
                ("swap-bytes", u64_datum(swap_bytes)),
            ],
        ),
    }
}
fn filesystem_limit_datum(limit: SandboxFilesystemLimit) -> Datum {
    match limit {
        SandboxFilesystemLimit::LogicalTree { entries, bytes } => node(
            "filesystem-logical-tree-v1",
            vec![
                ("entries", u64_datum(entries)),
                ("logical-bytes", u64_datum(bytes)),
            ],
        ),
        SandboxFilesystemLimit::Allocated { inodes, bytes } => node(
            "filesystem-allocated-v1",
            vec![
                ("inodes", u64_datum(inodes)),
                ("allocated-bytes", u64_datum(bytes)),
            ],
        ),
    }
}
pub(super) fn replay_datum(replay: CommandReplayPolicy) -> Datum {
    Datum::Symbol(Symbol::qualified(
        "operation",
        match replay {
            CommandReplayPolicy::Idempotent => "idempotent",
            CommandReplayPolicy::ExactlyOnce => "exactly-once",
        },
    ))
}
pub(crate) const fn control_name(control: SandboxControl) -> &'static str {
    match control {
        SandboxControl::Network => "network",
        SandboxControl::Mounts => "mounts",
        SandboxControl::Root => "root",
        SandboxControl::Environment => "environment",
        SandboxControl::Identity => "identity",
        SandboxControl::Cpu => "cpu",
        SandboxControl::Memory => "memory",
        SandboxControl::WallTime => "wall-time",
        SandboxControl::ProcessCount => "process-count",
        SandboxControl::FileCount => "file-count",
        SandboxControl::FileBytes => "file-bytes",
        SandboxControl::Output => "output",
        SandboxControl::Stdin => "stdin",
        SandboxControl::ProcessTree => "process-tree",
    }
}
fn requirement_name(requirement: SandboxRequirement) -> &'static str {
    match requirement {
        SandboxRequirement::Required => "required",
        SandboxRequirement::BestEffort => "best-effort",
    }
}
