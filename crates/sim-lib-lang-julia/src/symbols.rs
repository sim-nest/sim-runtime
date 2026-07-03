use sim_kernel::Symbol;

/// Stable symbol identifying the Julia core language profile.
pub fn julia_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "julia-core/v1")
}

/// Stable symbol for the reader codec the Julia surface decodes through.
///
/// The Julia profile reuses the shared algol reader rather than a bespoke one.
pub fn julia_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "algol")
}

/// Stable symbol for the Julia lowering from surface forms to `Expr`.
pub fn julia_lowering_symbol() -> Symbol {
    Symbol::qualified("julia", "lowering-core")
}

/// Stable symbol for the Julia core dispatch conformance test.
pub fn julia_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "julia-core-dispatch")
}

/// Stable symbol for the Julia dispatch-organ fidelity badge.
pub fn julia_dispatch_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "julia-dispatch-organ")
}

/// Stable symbol for the Julia full-runtime fidelity badge (limited support).
pub fn julia_full_runtime_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "julia-full-runtime-limited")
}
