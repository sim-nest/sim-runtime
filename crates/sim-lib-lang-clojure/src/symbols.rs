use sim_kernel::Symbol;

/// Stable symbol identifying the Clojure-core language profile.
pub fn clojure_core_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "clojure-core/v1")
}

/// Stable symbol under which the EDN reader codec is registered.
pub fn clojure_edn_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "clojure-edn")
}

/// Stable symbol identifying the Clojure-core lowering pass.
pub fn clojure_lowering_symbol() -> Symbol {
    Symbol::qualified("clojure", "lowering-core")
}

/// Stable symbol identifying the Clojure-core organ conformance test.
pub fn clojure_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "clojure-core-organs")
}

/// Stable symbol for the sequence-organ fidelity badge of this profile.
pub fn clojure_sequence_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "clojure-sequence-organ")
}

/// Stable symbol for the namespace-organ fidelity badge of this profile.
pub fn clojure_namespace_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "clojure-namespace-organ")
}

/// Stable symbol for the control-organ fidelity badge of this profile.
pub fn clojure_control_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "clojure-control-organ")
}

/// Stable symbol naming the generated `clojure.core` namespace.
pub fn clojure_core_namespace_symbol() -> Symbol {
    Symbol::qualified("clojure", "core")
}

/// Stable symbol naming the control prompt used by `loop`/`recur`.
pub fn clojure_recur_prompt_symbol() -> Symbol {
    Symbol::qualified("clojure", "loop-recur")
}
