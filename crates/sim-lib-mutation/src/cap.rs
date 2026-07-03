use sim_kernel::CapabilityName;

/// The capability name every mutation write requires: `standard.mutate`.
///
/// In-place writes on cells, boxes, vectors, and tables call `cx.require` with
/// this name, so mutation fails closed unless the context has been granted the
/// capability.
pub fn standard_mutate_capability() -> CapabilityName {
    CapabilityName::new("standard.mutate")
}
