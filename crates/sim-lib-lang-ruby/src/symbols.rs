use sim_kernel::Symbol;

/// Stable symbol identifying the Ruby DSL language profile.
pub fn ruby_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "ruby-dsl/v1")
}

/// Stable symbol for the reader codec the Ruby surface decodes through.
///
/// The Ruby profile reuses the shared algol reader rather than a bespoke one.
pub fn ruby_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "algol")
}

/// Stable symbol for the Ruby lowering from surface forms to `Expr`.
pub fn ruby_lowering_symbol() -> Symbol {
    Symbol::qualified("ruby", "lowering-dsl")
}

/// Stable symbol for the Ruby DSL control conformance test.
pub fn ruby_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "ruby-dsl-control")
}

/// Stable symbol for the Ruby control-blocks fidelity badge.
pub fn ruby_control_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "ruby-control-blocks")
}

/// Stable symbol for the Ruby method-dispatch fidelity badge.
pub fn ruby_dispatch_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "ruby-dispatch-methods")
}

/// Stable symbol for the Ruby full-blocks fidelity badge (limited support).
pub fn ruby_blocks_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "ruby-full-blocks-limited")
}
