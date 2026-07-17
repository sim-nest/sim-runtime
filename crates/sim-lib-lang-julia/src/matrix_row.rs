//! Julia conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

use crate::{julia_core_profile, julia_lowering_symbol, julia_reader_symbol};

/// Builds the Julia core matrix row.
pub fn julia_core_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("julia"), julia_core_profile())
        .with_cases(julia_core_source_cases())
        .build()
}

/// Minimal source cases for the Julia core matrix row.
pub fn julia_core_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/julia-core", "profile-declared"),
            organ: julia_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::LowersTo(julia_core_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/julia-core", "runtime-gap"),
            organ: julia_lowering_symbol(),
            source_name: "runtime-gap.jl".to_owned(),
            source: "eval(:(1 + 2))".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("julia", "runtime-gap"),
                reason: "Julia world-age runtime execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn julia_core_profile_display() -> String {
    let profile = julia_core_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn julia_core_matrix_row_language_symbol_is_julia() {
        let row = julia_core_matrix_row();

        assert_eq!(row.language, Symbol::new("julia"));
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
