//! Explicit resource-accounting units shared by sandbox requests and evidence.

use sim_kernel::{Error, Result};

/// CPU bounds with distinct duration and scheduling-rate semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxCpuLimit {
    /// CPU seconds available to each process; not aggregate job CPU time.
    PerProcessSeconds(u64),
    /// Aggregate execution allowance per scheduling period, in microseconds.
    ///
    /// The period is part of the contract: equal ratios with different periods
    /// need not permit the same bursts. This is not a total CPU-time budget.
    Rate {
        /// Aggregate CPU microseconds available during each period.
        quota_us: u64,
        /// Scheduling period in monotonic microseconds.
        period_us: u64,
    },
}

/// Memory bounds distinguish virtual address space from aggregate charged usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxMemoryLimit {
    /// Virtual address-space bytes available to each process.
    PerProcessAddressSpaceBytes(u64),
    /// Aggregate memory and swap charged to the job's resource domain.
    Charged {
        /// Maximum charged memory bytes, excluding swapped-out bytes.
        bytes: u64,
        /// Separate swap-byte maximum; zero explicitly prohibits swap.
        swap_bytes: u64,
    },
}

/// Aggregate filesystem bounds; accounting variants are not interchangeable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxFilesystemLimit {
    /// Logical usage across declared writable roots.
    ///
    /// Non-directory entries are counted by path, including symbolic links.
    /// Byte usage sums non-symlink file lengths; aliases may count repeatedly.
    /// Physical quotas do not prove this bound, including for sparse files.
    LogicalTree {
        /// Maximum non-directory entries across the writable roots.
        entries: u64,
        /// Maximum summed logical file lengths, in bytes.
        bytes: u64,
    },
    /// Allocations in one shared filesystem quota domain for all writable roots.
    ///
    /// Includes preexisting usage and directory inodes. A hard-linked inode is
    /// counted once. Allocated bytes are not a bound on sparse logical lengths.
    Allocated {
        /// Maximum distinct allocated inodes, including directories.
        inodes: u64,
        /// Maximum filesystem bytes charged to the quota domain.
        bytes: u64,
    },
}

/// Complete resource policy with explicit accounting units.
///
/// Every bound must be positive except the explicit swap maximum, which may
/// be zero. Providers refuse Required controls they cannot enforce; a final
/// usage observation alone never establishes prospective enforcement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxLimits {
    /// CPU duration or rate, with its explicit scope.
    pub cpu: SandboxCpuLimit,
    /// Address space or aggregate charged memory, with explicit swap policy.
    pub memory: SandboxMemoryLimit,
    /// Monotonic milliseconds.
    pub wall_time_ms: u64,
    /// Maximum descendant tasks in the job resource domain.
    pub process_count: u64,
    /// Aggregate filesystem bound and accounting units.
    pub filesystem: SandboxFilesystemLimit,
    /// Shared stdout and stderr cap.
    pub output_bytes: usize,
    /// Standard-input cap.
    pub stdin_bytes: usize,
}

impl SandboxLimits {
    pub(super) fn validate(&self, max_stdin: usize) -> Result<()> {
        let cpu_valid = match self.cpu {
            SandboxCpuLimit::PerProcessSeconds(seconds) => seconds > 0,
            SandboxCpuLimit::Rate {
                quota_us,
                period_us,
            } => quota_us > 0 && period_us > 0,
        };
        let memory_valid = match self.memory {
            SandboxMemoryLimit::PerProcessAddressSpaceBytes(bytes)
            | SandboxMemoryLimit::Charged { bytes, .. } => bytes > 0,
        };
        let filesystem_valid = match self.filesystem {
            SandboxFilesystemLimit::LogicalTree { entries, bytes } => entries > 0 && bytes > 0,
            SandboxFilesystemLimit::Allocated { inodes, bytes } => inodes > 0 && bytes > 0,
        };
        if !cpu_valid
            || !memory_valid
            || !filesystem_valid
            || self.wall_time_ms == 0
            || self.process_count == 0
            || self.output_bytes == 0
            || self.stdin_bytes == 0
            || self.stdin_bytes > max_stdin
        {
            return Err(Error::Eval(
                "sandbox limits must have valid explicit accounting and bounds".into(),
            ));
        }
        Ok(())
    }
}
