use sim_kernel::Symbol;

/// Stable symbol identifying the typed, lazy language profile.
pub fn typed_lazy_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "typed-lazy/v1")
}

/// Stable symbol of the reader codec this profile uses for its surface syntax.
pub fn typed_lazy_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "algol")
}

/// Stable symbol identifying the typed-lazy lowering pass.
pub fn typed_lazy_lowering_symbol() -> Symbol {
    Symbol::qualified("typed-lazy", "lowering-core")
}

/// Stable symbol identifying the typed-lazy organ conformance test.
pub fn typed_lazy_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "typed-lazy-patterns")
}

/// Stable symbol for the pattern/ADT fidelity badge of this profile.
pub fn typed_lazy_pattern_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "typed-lazy-pattern-adts")
}

/// Stable symbol for the (limited) laziness fidelity badge of this profile.
pub fn typed_lazy_control_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "typed-lazy-laziness-limited")
}

/// Stable symbol for the (limited) typeclass fidelity badge of this profile.
pub fn typed_lazy_typeclass_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "typed-lazy-typeclasses-limited")
}
