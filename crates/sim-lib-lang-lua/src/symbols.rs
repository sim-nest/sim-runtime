use sim_kernel::Symbol;

/// Stable symbol identifying the Lua core language profile.
pub fn lua_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "lua-core/v1")
}

/// Stable symbol for the reader codec the Lua surface decodes through.
///
/// The Lua profile reuses the shared algol reader rather than a bespoke one.
pub fn lua_reader_symbol() -> Symbol {
    Symbol::qualified("codec", "algol")
}

/// Stable symbol for the Lua lowering from surface forms to `Expr`.
pub fn lua_lowering_symbol() -> Symbol {
    Symbol::qualified("lua", "lowering-core")
}

/// Stable symbol for the Lua core control/mutation conformance test.
pub fn lua_conformance_test_symbol() -> Symbol {
    Symbol::qualified("test", "lua-core-control-mutation")
}

/// Stable symbol for the Lua coroutine-control fidelity badge.
pub fn lua_control_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "lua-control-coroutines")
}

/// Stable symbol for the Lua table-mutation fidelity badge.
pub fn lua_mutation_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "lua-mutation-tables")
}

/// Stable symbol for the Lua full-runtime fidelity badge (limited support).
pub fn lua_full_runtime_fidelity_symbol() -> Symbol {
    Symbol::qualified("standard/fidelity", "lua-full-runtime-limited")
}
