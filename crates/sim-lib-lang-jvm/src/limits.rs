use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_machine::AdmissionLimits;

/// Limits which bound admitted JVM execution shape and work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionLimits {
    /// Shared neutral-machine limits.
    pub machine: AdmissionLimits,
    /// Maximum class hierarchy depth inspected by one operation.
    pub class_depth: AdmissionLimit,
    /// Maximum exception-table entries inspected by one dispatch.
    pub handler_entries: WorkLimit,
}

/// Limits for resources retained outside an individual machine drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum admitted classfile bytes.
    pub classfile_bytes: AdmissionLimit,
    /// Maximum managed objects attributable to one JVM instance.
    pub managed_objects: AdmissionLimit,
    /// Maximum interned strings attributable to one JVM instance.
    pub interned_strings: AdmissionLimit,
}
