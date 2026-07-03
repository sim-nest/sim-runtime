use sim_kernel::Symbol;

/// Stable identity symbol for the Common Lisp (lite) language profile.
pub fn cl_lite_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "common-lisp-lite/v1")
}

/// Codec symbol naming the CL-lite surface reader.
pub fn cl_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "common-lisp-lite")
}

/// Symbol naming the CL-lite surface-to-`Expr` lowering pass.
pub fn cl_lowering_symbol() -> Symbol {
    Symbol::qualified("cl", "lowering-lite")
}

/// Symbol naming the profile's organ conformance test.
pub fn cl_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "common-lisp-lite-organs")
}

/// Fidelity-badge symbol for the binding-organ surface coverage.
pub fn cl_binding_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-binding-organ")
}

/// Fidelity-badge symbol for the control-organ surface coverage.
pub fn cl_control_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-control-organ")
}

/// Fidelity-badge symbol for the dispatch-organ surface coverage.
pub fn cl_dispatch_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-dispatch-organ")
}

/// Fidelity-badge symbol for the namespace-organ surface coverage.
pub fn cl_namespace_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-namespace-organ")
}

/// Fidelity-badge symbol for the mutation-organ surface coverage.
pub fn cl_mutation_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-mutation-organ")
}

/// Fidelity-badge symbol marking the limited CLOS/MOP surface.
pub fn cl_clos_mop_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "cl-lite-clos-mop-limited")
}

/// Symbol naming the CL-lite package namespace.
pub fn cl_lite_package_symbol() -> Symbol {
    Symbol::qualified("common-lisp", "lite")
}
