use sim_kernel::Symbol;

/// Stable symbol identifying the Prolog surface language profile.
pub fn prolog_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "prolog/v1")
}

/// Stable symbol identifying the expression reader used by this surface.
pub fn prolog_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

/// Stable symbol identifying the Prolog surface lowering path.
pub fn prolog_lowering_symbol() -> Symbol {
    Symbol::qualified("prolog", "surface-expr")
}

/// Stable symbol identifying the Prolog conformance test.
pub fn prolog_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "prolog-surface-core")
}

/// Stable symbol for the Prolog surface fidelity badge.
pub fn prolog_surface_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "prolog-surface-partial")
}

/// Stable symbol identifying the logic organ used by the Prolog surface.
pub fn prolog_logic_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "logic")
}

/// Stable symbol identifying a Prolog matrix source case.
pub fn prolog_conformance_case_symbol(name: &str) -> Symbol {
    Symbol::qualified("test/prolog", name.to_owned())
}
