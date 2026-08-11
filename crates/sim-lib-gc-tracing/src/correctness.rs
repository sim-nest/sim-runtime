/// Independent correctness dimensions maintained by the collector conformance suite.
///
/// Determinism means identical allocation and safepoint schedules produce identical
/// receipts. Receipt order is grounded in `ManagedId`'s allocation-deterministic
/// ordinals; safety does not require different legal schedules to reclaim at the
/// same safepoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrectnessDimension {
    /// No rooted or transitively strong-reachable object is reclaimed.
    Safety,
    /// Unreachable strong cycles are reclaimed.
    Reclamation,
    /// Weak edges and ephemerons follow strong-liveness semantics.
    WeakAndEphemeron,
    /// Finalizers are admitted at most once and run outside collection.
    Finalization,
    /// The same allocation and safepoint schedule gives the same receipt.
    SameScheduleDeterminism,
    /// Every legal schedule preserves safety, even when reclamation timing differs.
    ScheduleIndependenceSafety,
    /// Every collection resource class is explicitly bounded.
    BoundedWork,
    /// Refused plans leave the arena and finalization state unchanged.
    FailureAtomicity,
    /// Collection runs synchronously inside a wasm-compatible closure without threads.
    WasmClosure,
}

impl CorrectnessDimension {
    /// The complete frozen correctness contract, in documentation order.
    pub const ALL: [Self; 9] = [
        Self::Safety,
        Self::Reclamation,
        Self::WeakAndEphemeron,
        Self::Finalization,
        Self::SameScheduleDeterminism,
        Self::ScheduleIndependenceSafety,
        Self::BoundedWork,
        Self::FailureAtomicity,
        Self::WasmClosure,
    ];
}
