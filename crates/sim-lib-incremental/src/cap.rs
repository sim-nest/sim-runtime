//! Capability names required by the incremental organ.

use sim_kernel::CapabilityName;

/// Capability required for read-only incremental reports and metrics.
pub fn incremental_read_capability() -> CapabilityName {
    CapabilityName::new("incremental.read")
}

/// Capability required for registering queries or invalidating observed keys.
pub fn incremental_write_capability() -> CapabilityName {
    CapabilityName::new("incremental.write")
}

/// Capability required for executing query verification.
pub fn incremental_verify_capability() -> CapabilityName {
    CapabilityName::new("incremental.verify")
}
