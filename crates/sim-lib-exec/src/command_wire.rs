//! Canonical semantic projection for exact command contracts.

use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};

use crate::{
    BindingValue, ProcessBudget, SandboxControl, SandboxRequirement, SealedBindings,
    command::{
        CommandInvocation, CommandReplayPolicy, CommandResource, CommandRoute, NetworkAccess,
        OutputExpectation, OutputState, ResourceAccess,
    },
};

pub(super) fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("local-check", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}
pub(super) fn id_datum(id: &ContentId) -> Datum {
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
fn u64_datum(value: u64) -> Datum {
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
            "sandbox-route-v1",
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
                        "limits-v1",
                        vec![
                            ("cpu-seconds", u64_datum(policy.limits().cpu_seconds)),
                            ("memory-bytes", u64_datum(policy.limits().memory_bytes)),
                            ("wall-time-ms", u64_datum(policy.limits().wall_time_ms)),
                            ("process-count", u64_datum(policy.limits().process_count)),
                            ("file-count", u64_datum(policy.limits().file_count)),
                            ("file-bytes", u64_datum(policy.limits().file_bytes)),
                            ("output-bytes", usize_datum(policy.limits().output_bytes)),
                            ("stdin-bytes", usize_datum(policy.limits().stdin_bytes)),
                        ],
                    ),
                ),
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
fn control_name(control: SandboxControl) -> &'static str {
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
