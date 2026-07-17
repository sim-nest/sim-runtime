//! Lua conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

use crate::{lua_core_profile, lua_lowering_symbol, lua_reader_symbol};

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
        SourceConformanceCase {
            symbol: Symbol::qualified("test/lua-core", "runtime-gap"),
            organ: lua_lowering_symbol(),
            source_name: "runtime-gap.lua".to_owned(),
            source: "return load('return 1 + 2')()".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("lua", "runtime-gap"),
                reason: "Lua full VM execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
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
        assert_eq!(row.cases.len(), 2);
        assert!(matches!(
            row.cases[0].expectation,
            SourceExpectation::LowersTo(_)
        ));
        assert!(matches!(
            row.cases[1].expectation,
            SourceExpectation::ExpectedGap { .. }
        ));
    }
}
