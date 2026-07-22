//! Lua conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

use crate::{
    lua_core_profile, lua_full_runtime_fidelity_symbol, lua_lowering_symbol, lua_reader_symbol,
};

/// Builds the Lua core matrix row.
pub fn lua_core_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("lua"), lua_core_profile())
        .with_cases(lua_core_source_cases())
        .build()
}

/// Minimal source cases for the Lua core matrix row.
pub fn lua_core_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/lua-core", "profile-declared"),
            organ: lua_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::LowersTo(lua_core_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        observed("load-source", "return load('return 1 + 2')()", "3"),
        observed(
            "closure-upvalue",
            "local n = 0 local f = function() n = n + 1 return n end return f() + f()",
            "3",
        ),
        observed(
            "metatable-vector",
            "local mt = { __add = function(a, b) return { x = a.x + b.x, y = a.y + b.y } end } local a = setmetatable({ x = 2, y = 4 }, mt) local b = { x = 3, y = 5 } local c = a + b return c.x + c.y",
            "14",
        ),
        observed(
            "coroutine-producer",
            "local co = coroutine.create(function() return 1 end) return select(2, coroutine.resume(co))",
            "1",
        ),
        observed(
            "string-patterns",
            "local out, n = string.gsub('aba', 'a', 'x') return out .. ':' .. n",
            "xbx:2",
        ),
        expected_gap(
            "bytecode",
            "return string.dump(function() end)",
            "lua.bytecode.dump",
            "Lua bytecode dumping is explicitly unsupported",
        ),
        expected_gap(
            "debug-hook",
            "return debug.sethook(function() end, 'l')",
            "lua.debug.sethook",
            "Lua debug hooks are outside the safe debug subset",
        ),
        expected_gap(
            "c-api",
            "return package.loadlib('liblua.so', 'luaopen_demo')",
            "lua.c-api",
            "Lua C API package loading is outside the source runtime",
        ),
    ]
}

fn observed(name: &str, source: &str, expected: &str) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: Symbol::qualified("test/lua-core", name),
        organ: lua_lowering_symbol(),
        source_name: format!("{name}.lua"),
        source: source.to_owned(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::LowersTo(expected.to_owned()),
        affects_badge: Some(lua_full_runtime_fidelity_symbol()),
    }
}

fn expected_gap(name: &str, source: &str, code: &str, reason: &str) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: Symbol::qualified("test/lua-core", name),
        organ: lua_lowering_symbol(),
        source_name: format!("{name}.lua"),
        source: source.to_owned(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::ExpectedGap {
            code: Symbol::new(code),
            reason: reason.to_owned(),
        },
        affects_badge: Some(lua_full_runtime_fidelity_symbol()),
    }
}

fn lua_core_profile_display() -> String {
    let profile = lua_core_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_core_matrix_row_language_symbol_is_lua() {
        let row = lua_core_matrix_row();

        assert_eq!(row.language, Symbol::new("lua"));
        assert!(!row.is_empty());
        assert_eq!(row.cases.len(), 9);
        assert!(matches!(
            row.cases[0].expectation,
            SourceExpectation::LowersTo(_)
        ));
        assert!(matches!(
            row.cases[1].expectation,
            SourceExpectation::LowersTo(_)
        ));
        assert!(
            row.cases
                .iter()
                .any(|case| matches!(case.expectation, SourceExpectation::ExpectedGap { .. }))
        );
    }
}
