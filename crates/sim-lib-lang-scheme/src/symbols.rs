use sim_kernel::Symbol;

/// Stable identity symbol for the R7RS-small language profile.
pub fn r7rs_small_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "scheme-r7rs-small/v1")
}

/// Codec symbol naming the Scheme surface reader.
pub fn scheme_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "scheme-r7rs-small")
}

/// Symbol naming the Scheme surface-to-`Expr` lowering pass.
pub fn scheme_lowering_symbol() -> Symbol {
    Symbol::qualified("scheme", "lower-r7rs-small")
}

/// Symbol naming the profile's conformance test.
pub fn scheme_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "scheme-r7rs-small-core")
}

/// Card-kind symbol tagging published Scheme base-library exports.
pub fn scheme_base_export_kind_symbol() -> Symbol {
    Symbol::qualified("scheme", "base-export")
}

/// Builds a `scheme`-qualified symbol for the given surface name.
pub fn scheme_symbol(name: &str) -> Symbol {
    Symbol::qualified("scheme", name.to_owned())
}
