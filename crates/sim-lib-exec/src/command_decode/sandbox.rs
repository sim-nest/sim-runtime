//! Decode complete portable sandbox controls through their existing constructors.

use super::*;
use crate::{
    MountAccess, SandboxCpuLimit, SandboxFilesystemLimit, SandboxLimits, SandboxMemoryLimit,
    SandboxMount,
};

pub(super) fn route(value: &Datum) -> Result<CommandRoute> {
    if matches!(value, Datum::Symbol(_)) {
        if token(value, "command-route")? != "process" {
            return Err(invalid("command route"));
        }
        return Ok(CommandRoute::Process);
    }
    let f = fields(value, "sandbox-route-v2", 4)?;
    let Datum::Map(entries) = field(f, "requirements")? else {
        return Err(invalid("requirements map"));
    };
    let requirements = entries
        .iter()
        .map(|(control, requirement)| {
            let name = token(control, "sandbox-control")?;
            let control = SandboxControl::ALL
                .into_iter()
                .find(|control| crate::command_wire::control_name(*control) == name)
                .ok_or_else(|| invalid("sandbox control"))?;
            let requirement = match token(requirement, "sandbox-requirement")? {
                "required" => SandboxRequirement::Required,
                "best-effort" => SandboxRequirement::BestEffort,
                _ => return Err(invalid("sandbox requirement")),
            };
            Ok((control, requirement))
        })
        .collect::<Result<Vec<_>>>()?;
    let mounts = vector(field(f, "mounts")?)?
        .iter()
        .map(|value| {
            let f = fields(value, "mount-v1", 3)?;
            let access = match token(field(f, "access")?, "mount-access")? {
                "read-only" => MountAccess::ReadOnly,
                "writable" => MountAccess::Writable,
                _ => return Err(invalid("mount access")),
            };
            Ok(SandboxMount {
                source: string(field(f, "source")?)?.into(),
                guest_path: string(field(f, "guest-path")?)?.into(),
                access,
            })
        })
        .collect::<Result<_>>()?;
    Ok(CommandRoute::Sandbox {
        launcher: string(field(f, "launcher")?)?.into(),
        policy: SandboxPolicy::new(requirements, mounts, limits(field(f, "limits")?)?)?,
    })
}

fn limits(value: &Datum) -> Result<SandboxLimits> {
    let f = fields(value, "limits-v2", 7)?;
    Ok(SandboxLimits {
        cpu: cpu(field(f, "cpu")?)?,
        memory: memory(field(f, "memory")?)?,
        filesystem: filesystem(field(f, "filesystem")?)?,
        wall_time_ms: unsigned(field(f, "wall-time-ms")?)?,
        process_count: unsigned(field(f, "process-count")?)?,
        output_bytes: size(field(f, "output-bytes")?)?,
        stdin_bytes: size(field(f, "stdin-bytes")?)?,
    })
}

fn cpu(value: &Datum) -> Result<SandboxCpuLimit> {
    match tag(value)? {
        "cpu-per-process-time-v1" => {
            let f = fields(value, "cpu-per-process-time-v1", 1)?;
            Ok(SandboxCpuLimit::PerProcessSeconds(unsigned(field(
                f, "seconds",
            )?)?))
        }
        "cpu-aggregate-rate-v1" => {
            let f = fields(value, "cpu-aggregate-rate-v1", 2)?;
            Ok(SandboxCpuLimit::Rate {
                quota_us: unsigned(field(f, "quota-us")?)?,
                period_us: unsigned(field(f, "period-us")?)?,
            })
        }
        _ => Err(invalid("CPU accounting")),
    }
}

fn memory(value: &Datum) -> Result<SandboxMemoryLimit> {
    match tag(value)? {
        "memory-per-process-address-space-v1" => {
            let f = fields(value, "memory-per-process-address-space-v1", 1)?;
            Ok(SandboxMemoryLimit::PerProcessAddressSpaceBytes(unsigned(
                field(f, "bytes")?,
            )?))
        }
        "memory-aggregate-charge-v1" => {
            let f = fields(value, "memory-aggregate-charge-v1", 2)?;
            Ok(SandboxMemoryLimit::Charged {
                bytes: unsigned(field(f, "bytes")?)?,
                swap_bytes: unsigned(field(f, "swap-bytes")?)?,
            })
        }
        _ => Err(invalid("memory accounting")),
    }
}

fn filesystem(value: &Datum) -> Result<SandboxFilesystemLimit> {
    match tag(value)? {
        "filesystem-logical-tree-v1" => {
            let f = fields(value, "filesystem-logical-tree-v1", 2)?;
            Ok(SandboxFilesystemLimit::LogicalTree {
                entries: unsigned(field(f, "entries")?)?,
                bytes: unsigned(field(f, "logical-bytes")?)?,
            })
        }
        "filesystem-allocated-v1" => {
            let f = fields(value, "filesystem-allocated-v1", 2)?;
            Ok(SandboxFilesystemLimit::Allocated {
                inodes: unsigned(field(f, "inodes")?)?,
                bytes: unsigned(field(f, "allocated-bytes")?)?,
            })
        }
        _ => Err(invalid("filesystem accounting")),
    }
}
