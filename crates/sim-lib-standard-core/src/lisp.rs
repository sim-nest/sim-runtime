//! Symbols exposed by the standard distribution on the lisp codec surface.

use sim_kernel::Symbol;

/// Lisp-surface symbol naming the standard profile entry point.
pub fn standard_profile_symbol() -> Symbol {
    Symbol::qualified("standard", "profile")
}

/// Lisp-surface symbol naming the standard fidelity entry point.
pub fn standard_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard", "fidelity")
}

/// The full set of symbols the standard distribution exposes on the lisp codec
/// surface (profile, fidelity, diff, install, test, and organ queries).
pub fn lisp_stub_symbols() -> Vec<Symbol> {
    vec![
        standard_profile_symbol(),
        standard_fidelity_symbol(),
        Symbol::qualified("profile", "diff"),
        Symbol::qualified("standard", "install"),
        Symbol::qualified("standard", "diff"),
        Symbol::qualified("standard", "test"),
        Symbol::qualified("organ", "list"),
        Symbol::qualified("organ", "describe"),
    ]
}
