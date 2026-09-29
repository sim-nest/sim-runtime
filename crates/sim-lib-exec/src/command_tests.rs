// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
            cpu: crate::SandboxCpuLimit::PerProcessSeconds(1),
            memory: crate::SandboxMemoryLimit::PerProcessAddressSpaceBytes(1024),
            wall_time_ms: 1000,
            process_count: 4,
            filesystem: crate::SandboxFilesystemLimit::LogicalTree {
                entries: 8,
                bytes: 4096,
            },
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

fn checkout_command() -> CommandSpec {
    CommandSpec::new(
        ProgramRef::new("formatter").unwrap(),
        ProjectRootRef::new("work").unwrap(),
        CommandInvocation::Argv(vec![ArgAtom::new("/work/src/lib.rs").unwrap()]),
        SealedBindings::empty(),
        vec![
            CommandResource {
                source: "work".into(),
                guest_path: "/work".into(),
                access: ResourceAccess::Writable,
            },
            CommandResource {
                source: "owner-source".into(),
                guest_path: "/source".into(),
                access: ResourceAccess::ReadOnly,
            },
        ],
        budget(),
        OutputContract::new(
            [0],
            vec![OutputExpectation {
                resource: "work".into(),
                relative_path: "src/lib.rs".into(),
                state: OutputState::Exists,
            }],
        )
        .unwrap(),
        CleanupContract::process_group(["work".into()]).unwrap(),
        NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
        CommandRoute::Process,
        CommandReplayPolicy::ExactlyOnce,
    )
    .unwrap()
}

fn seed(path: &str, target: &str) -> CheckoutFile {
    CheckoutFile {
        resource: "owner-source".into(),
        path: path.into(),
        target: target.into(),
    }
}

#[test]
fn checkout_is_bound_into_the_command_identity_and_round_trips() {
    let plain = checkout_command();
    assert!(plain.checkout().is_empty());
    assert_eq!(
        plain.canonical_datum(),
        CommandSpec::from_datum(&plain.canonical_datum())
            .unwrap()
            .canonical_datum()
    );
    let seeded = checkout_command()
        .with_checkout(vec![seed("lib.rs", "src/lib.rs")])
        .unwrap();
    let other = checkout_command()
        .with_checkout(vec![seed("other.rs", "src/lib.rs")])
        .unwrap();
    assert_ne!(plain.id(), seeded.id());
    assert_ne!(seeded.id(), other.id());
    let decoded = CommandSpec::from_datum(&seeded.canonical_datum()).unwrap();
    assert_eq!(decoded.id(), seeded.id());
    assert_eq!(decoded.checkout(), seeded.checkout());
    let reordered = checkout_command()
        .with_checkout(vec![seed("b.rs", "src/b.rs"), seed("a.rs", "src/a.rs")])
        .unwrap();
    assert_eq!(reordered.checkout()[0].path, "a.rs");
}

#[test]
fn checkout_refuses_writable_sources_escapes_duplicates_and_unbounded_plans() {
    let refused = |files: Vec<CheckoutFile>| checkout_command().with_checkout(files).is_err();
    assert!(refused(vec![]));
    assert!(refused(
        (0..=MAX_CHECKOUT_FILES)
            .map(|index| seed("lib.rs", &format!("src/{index}.rs")))
            .collect()
    ));
    assert!(refused(vec![CheckoutFile {
        resource: "work".into(),
        path: "lib.rs".into(),
        target: "src/lib.rs".into(),
    }]));
    assert!(refused(vec![CheckoutFile {
        resource: "missing".into(),
        path: "lib.rs".into(),
        target: "src/lib.rs".into(),
    }]));
    for (path, target) in [
        ("/lib.rs", "src/lib.rs"),
        ("lib.rs", "../lib.rs"),
        ("lib.rs", "src//lib.rs"),
        ("lib.rs", "./lib.rs"),
        ("lib\u{7}.rs", "src/lib.rs"),
        ("", "src/lib.rs"),
    ] {
        assert!(refused(vec![seed(path, target)]), "{path} -> {target}");
    }
    assert!(refused(vec![
        seed("a.rs", "src/lib.rs"),
        seed("b.rs", "src/lib.rs")
    ]));
    assert!(refused(vec![
        seed("a.rs", "src"),
        seed("b.rs", "src/lib.rs")
    ]));
    assert!(refused(vec![seed(
        "a.rs",
        ".sim-native-acknowledgement-canary-v1"
    )]));
    assert!(refused(vec![seed("a.rs", "src/.sim-replace-1-0")]));
    assert!(
        !refused(vec![seed("a.rs", "gen"), seed("b.rs", "gens/lib.rs")]),
        "a name prefix is not a directory"
    );
    let once = checkout_command()
        .with_checkout(vec![seed("lib.rs", "src/lib.rs")])
        .unwrap();
    assert!(once.with_checkout(vec![seed("a.rs", "src/a.rs")]).is_err());
}

#[test]
fn checkout_requires_a_writable_working_root() {
    let command = CommandSpec::new(
        ProgramRef::new("formatter").unwrap(),
        ProjectRootRef::new("owner-source").unwrap(),
        CommandInvocation::Argv(vec![]),
        SealedBindings::empty(),
        vec![
            CommandResource {
                source: "work".into(),
                guest_path: "/work".into(),
                access: ResourceAccess::Writable,
            },
            CommandResource {
                source: "owner-source".into(),
                guest_path: "/source".into(),
                access: ResourceAccess::ReadOnly,
            },
        ],
        budget(),
        OutputContract::new([0], vec![]).unwrap(),
        CleanupContract::process_group(["work".into()]).unwrap(),
        NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
        CommandRoute::Process,
        CommandReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    assert!(
        command
            .with_checkout(vec![seed("lib.rs", "src/lib.rs")])
            .is_err()
    );
}

fn manifest_command(script: &[u8]) -> CommandSpec {
    CommandSpec::new(
        ProgramRef::new("owner-shell").unwrap(),
        ProjectRootRef::new("work").unwrap(),
        CommandInvocation::Interpreter {
            flags: vec![ArgAtom::new("-c").unwrap()],
            script: script.to_vec(),
        },
        SealedBindings::empty(),
        vec![
            CommandResource {
                source: "work".into(),
                guest_path: "/work".into(),
                access: ResourceAccess::Writable,
            },
            CommandResource {
                source: "manifest".into(),
                guest_path: "/manifest".into(),
                access: ResourceAccess::ReadOnly,
            },
        ],
        budget(),
        OutputContract::new([0], vec![]).unwrap(),
        CleanupContract::process_group(["work".into()]).unwrap(),
        NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
        CommandRoute::Process,
        CommandReplayPolicy::ExactlyOnce,
    )
    .unwrap()
}

fn selection(field: &str) -> ManifestSelection {
    ManifestSelection {
        resource: "manifest".into(),
        path: "repos.toml".into(),
        table: "repo".into(),
        name: "sim-kernel".into(),
        field: field.into(),
    }
}

#[test]
fn manifest_selection_is_bound_into_the_command_identity_and_round_trips() {
    let plain = manifest_command(b"cargo test");
    let validation = manifest_command(b"cargo test")
        .with_manifest_selection(selection("validation_command"))
        .unwrap();
    let docs = manifest_command(b"cargo test")
        .with_manifest_selection(selection("docs_command"))
        .unwrap();
    assert_ne!(plain.id(), validation.id());
    assert_ne!(validation.id(), docs.id());
    assert_eq!(
        validation.manifest(),
        Some(&selection("validation_command"))
    );
    let decoded = CommandSpec::from_datum(&validation.canonical_datum()).unwrap();
    assert_eq!(decoded.id(), validation.id());
    assert_eq!(decoded.manifest(), validation.manifest());
    assert!(decoded.checkout().is_empty());
}

#[test]
fn manifest_selection_refuses_argv_commands_writable_manifests_and_loose_keys() {
    assert!(
        checkout_command()
            .with_manifest_selection(selection("validation_command"))
            .is_err(),
        "argv command"
    );
    let writable = ManifestSelection {
        resource: "work".into(),
        ..selection("validation_command")
    };
    assert!(
        manifest_command(b"x")
            .with_manifest_selection(writable)
            .is_err()
    );
    for field in ["", "validation command", "a/b", &"x".repeat(129)] {
        assert!(
            manifest_command(b"x")
                .with_manifest_selection(selection(field))
                .is_err(),
            "{field}"
        );
    }
    let bad_path = ManifestSelection {
        path: "../repos.toml".into(),
        ..selection("validation_command")
    };
    assert!(
        manifest_command(b"x")
            .with_manifest_selection(bad_path)
            .is_err()
    );
    let once = manifest_command(b"x")
        .with_manifest_selection(selection("validation_command"))
        .unwrap();
    assert!(
        once.with_manifest_selection(selection("docs_command"))
            .is_err()
    );
}

#[test]
fn checkout_refuses_outputs_above_or_inside_a_target() {
    let with_output = |path: &str| {
        CommandSpec::new(
            ProgramRef::new("formatter").unwrap(),
            ProjectRootRef::new("work").unwrap(),
            CommandInvocation::Argv(vec![]),
            SealedBindings::empty(),
            vec![
                CommandResource {
                    source: "work".into(),
                    guest_path: "/work".into(),
                    access: ResourceAccess::Writable,
                },
                CommandResource {
                    source: "owner-source".into(),
                    guest_path: "/source".into(),
                    access: ResourceAccess::ReadOnly,
                },
            ],
            budget(),
            OutputContract::new(
                [0],
                vec![OutputExpectation {
                    resource: "work".into(),
                    relative_path: path.into(),
                    state: OutputState::Exists,
                }],
            )
            .unwrap(),
            CleanupContract::process_group(["work".into()]).unwrap(),
            NetworkAccess::Scoped(CapabilityName::new("network/none-used")),
            CommandRoute::Process,
            CommandReplayPolicy::ExactlyOnce,
        )
        .unwrap()
        .with_checkout(vec![seed("lib.rs", "src/lib.rs")])
    };
    assert!(with_output("src").is_err());
    assert!(with_output("src/lib.rs/inner").is_err());
    assert!(with_output("src/lib.rs").is_ok());
    assert!(with_output("srcs").is_ok());
}
