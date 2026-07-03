use sim_kernel::Symbol;

/// Stable symbol identifying the ISLISP language profile.
pub fn islisp_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "islisp-core/v1")
}

/// Stable symbol for the ISLISP reader (surface decoder) codec.
pub fn islisp_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "islisp")
}

/// Stable symbol for the ISLISP lowering from surface forms to `Expr`.
pub fn islisp_lowering_symbol() -> Symbol {
    Symbol::qualified("islisp", "lowering-core")
}

/// Stable symbol for the ISLISP generics conformance test.
pub fn islisp_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "islisp-core-generics")
}

/// Stable symbol for the ISLISP dispatch-organ fidelity badge.
pub fn islisp_dispatch_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "islisp-dispatch-organ")
}
