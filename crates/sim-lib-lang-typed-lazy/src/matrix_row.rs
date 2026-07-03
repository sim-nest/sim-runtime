//! Typed-lazy conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceExpectation,
};

use crate::{typed_lazy_lowering_symbol, typed_lazy_profile, typed_lazy_reader_symbol};

/// Builds the typed-lazy matrix row.
pub fn typed_lazy_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("typed-lazy"), typed_lazy_profile())
        .with_cases(typed_lazy_source_cases())
        .build()
}

/// Minimal source cases for the typed-lazy matrix row.
pub fn typed_lazy_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/typed-lazy", "profile-declared"),
            organ: typed_lazy_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            expectation: SourceExpectation::LowersTo(typed_lazy_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/typed-lazy", "runtime-gap"),
            organ: typed_lazy_lowering_symbol(),
            source_name: "runtime-gap.tl".to_owned(),
            source: "force (delay 1)".to_owned(),
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("typed-lazy", "runtime-gap"),
                reason: "Typed-lazy full evaluator execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn typed_lazy_profile_display() -> String {
    let profile = typed_lazy_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lazy_matrix_row_language_symbol_is_typed_lazy() {
        let row = typed_lazy_matrix_row();

        assert_eq!(row.language, Symbol::new("typed-lazy"));
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
