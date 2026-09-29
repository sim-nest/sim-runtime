//! Resource units and every bound participate in exact command identity.

use crate::*;

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

pub(crate) fn limits() -> SandboxLimits {
    SandboxLimits {
        cpu: SandboxCpuLimit::PerProcessSeconds(10),
        memory: SandboxMemoryLimit::PerProcessAddressSpaceBytes(4096),
        filesystem: SandboxFilesystemLimit::LogicalTree {
            entries: 8,
            bytes: 4096,
        },
        wall_time_ms: 1000,
        process_count: 8,
        output_bytes: 4096,
        stdin_bytes: 1,
    }
}

pub(crate) fn command(limits: SandboxLimits) -> CommandSpec {
    let policy = SandboxPolicy::new(
        controls(),
        vec![SandboxMount {
            source: "work".into(),
            guest_path: "/work".into(),
            access: MountAccess::ReadOnly,
        }],
        limits.clone(),
    )
    .unwrap();
    CommandSpec::new(
        ProgramRef::new("checker").unwrap(),
        ProjectRootRef::new("work").unwrap(),
        CommandInvocation::Argv(vec![]),
        SealedBindings::empty(),
        vec![CommandResource {
            source: "work".into(),
            guest_path: "/work".into(),
            access: ResourceAccess::ReadOnly,
        }],
        ProcessBudget {
            timeout_ms: limits.wall_time_ms,
            max_output_bytes: limits.output_bytes,
            stdin: None,
        },
        OutputContract::new([0], vec![]).unwrap(),
        CleanupContract::process_group([]).unwrap(),
        NetworkAccess::Absent,
        CommandRoute::Sandbox {
            launcher: "fixture".into(),
            policy,
        },
        CommandReplayPolicy::Idempotent,
    )
    .unwrap()
}

#[test]
fn identical_quantities_with_different_accounting_have_different_command_ids() {
    let original = command(limits());
    assert_eq!(original.id(), command(limits()).id());
    let mut changed = limits();
    changed.filesystem = SandboxFilesystemLimit::Allocated {
        inodes: 8,
        bytes: 4096,
    };
    assert_ne!(original.id(), command(changed).id());
    let mut changed = limits();
    changed.memory = SandboxMemoryLimit::Charged {
        bytes: 4096,
        swap_bytes: 0,
    };
    assert_ne!(original.id(), command(changed).id());
    let mut changed = limits();
    changed.cpu = SandboxCpuLimit::Rate {
        quota_us: 10,
        period_us: 10,
    };
    assert_ne!(original.id(), command(changed).id());
}

#[test]
fn cpu_period_swap_and_each_filesystem_dimension_are_independent_inputs() {
    let mut physical = limits();
    physical.cpu = SandboxCpuLimit::Rate {
        quota_us: 50_000,
        period_us: 100_000,
    };
    physical.memory = SandboxMemoryLimit::Charged {
        bytes: 4096,
        swap_bytes: 0,
    };
    physical.filesystem = SandboxFilesystemLimit::Allocated {
        inodes: 8,
        bytes: 4096,
    };
    let expected = command(physical.clone());
    let mut mutations = Vec::new();
    let mut value = physical.clone();
    value.cpu = SandboxCpuLimit::Rate {
        quota_us: 100_000,
        period_us: 200_000,
    };
    mutations.push(value); // Equal rate, different allowed burst/period.
    let mut value = physical.clone();
    value.memory = SandboxMemoryLimit::Charged {
        bytes: 4096,
        swap_bytes: 1,
    };
    mutations.push(value);
    let mut value = physical.clone();
    value.memory = SandboxMemoryLimit::Charged {
        bytes: 4097,
        swap_bytes: 0,
    };
    mutations.push(value);
    let mut value = physical.clone();
    value.filesystem = SandboxFilesystemLimit::Allocated {
        inodes: 9,
        bytes: 4096,
    };
    mutations.push(value);
    let mut value = physical.clone();
    value.filesystem = SandboxFilesystemLimit::Allocated {
        inodes: 8,
        bytes: 4097,
    };
    mutations.push(value);
    for changed in mutations {
        assert_ne!(expected.id(), command(changed).id());
    }
}

#[test]
fn zero_resource_bounds_refuse_but_zero_swap_is_an_explicit_valid_limit() {
    let mut invalid = Vec::new();
    for cpu in [
        SandboxCpuLimit::PerProcessSeconds(0),
        SandboxCpuLimit::Rate {
            quota_us: 0,
            period_us: 1,
        },
        SandboxCpuLimit::Rate {
            quota_us: 1,
            period_us: 0,
        },
    ] {
        let mut value = limits();
        value.cpu = cpu;
        invalid.push(value);
    }
    for memory in [
        SandboxMemoryLimit::PerProcessAddressSpaceBytes(0),
        SandboxMemoryLimit::Charged {
            bytes: 0,
            swap_bytes: 1,
        },
    ] {
        let mut value = limits();
        value.memory = memory;
        invalid.push(value);
    }
    for filesystem in [
        SandboxFilesystemLimit::LogicalTree {
            entries: 0,
            bytes: 1,
        },
        SandboxFilesystemLimit::LogicalTree {
            entries: 1,
            bytes: 0,
        },
        SandboxFilesystemLimit::Allocated {
            inodes: 0,
            bytes: 1,
        },
        SandboxFilesystemLimit::Allocated {
            inodes: 1,
            bytes: 0,
        },
    ] {
        let mut value = limits();
        value.filesystem = filesystem;
        invalid.push(value);
    }
    for value in invalid {
        assert!(SandboxPolicy::new(controls(), vec![], value).is_err());
    }
    let mut valid = limits();
    valid.memory = SandboxMemoryLimit::Charged {
        bytes: 1,
        swap_bytes: 0,
    };
    assert!(SandboxPolicy::new(controls(), vec![], valid).is_ok());
}

#[test]
fn duplicate_control_classifications_refuse_instead_of_silently_overwriting() {
    for requirement in [SandboxRequirement::Required, SandboxRequirement::BestEffort] {
        assert!(
            SandboxPolicy::new(
                controls().chain([(SandboxControl::Memory, requirement)]),
                vec![],
                limits(),
            )
            .is_err()
        );
    }
}

#[test]
fn duplicate_or_blank_control_evidence_cannot_prove_a_required_bound() {
    let policy = SandboxPolicy::new(controls(), vec![], limits()).unwrap();
    let complete = SandboxReport {
        launcher: "fixture".into(),
        controls: controls()
            .map(|(control, _)| SandboxEvidence {
                control,
                achieved: true,
                detail: "synthetic control evidence".into(),
            })
            .collect(),
        limit_hits: vec![],
        cleanup: "synthetic quiescence".into(),
    };
    assert!(complete.proves_required(&policy));
    for achieved in [false, true] {
        for insert_first in [false, true] {
            let mut report = complete.clone();
            let index = if insert_first {
                0
            } else {
                report.controls.len()
            };
            report.controls.insert(
                index,
                SandboxEvidence {
                    control: SandboxControl::Memory,
                    achieved,
                    detail: "duplicate claim".into(),
                },
            );
            assert!(!report.proves_required(&policy));
        }
    }
    for control in policy.requirements().keys() {
        let mut report = complete.clone();
        report
            .controls
            .iter_mut()
            .find(|e| e.control == *control)
            .unwrap()
            .detail = " \t\n".into();
        assert!(!report.proves_required(&policy));
    }
}
