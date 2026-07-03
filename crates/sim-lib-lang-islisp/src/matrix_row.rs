//! ISLISP conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceExpectation,
};

use crate::{islisp_lowering_symbol, islisp_profile, islisp_reader_symbol};

/// Builds the ISLISP matrix row.
pub fn islisp_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("islisp"), islisp_profile())
        .with_cases(islisp_source_cases())
        .build()
}

/// Minimal source cases for the ISLISP matrix row.
pub fn islisp_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/islisp", "profile-declared"),
            organ: islisp_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            expectation: SourceExpectation::LowersTo(islisp_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/islisp", "runtime-gap"),
            organ: islisp_lowering_symbol(),
            source_name: "runtime-gap.lisp".to_owned(),
            source: "(eval '(+ 1 2))".to_owned(),
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("islisp", "runtime-gap"),
                reason: "ISLISP full evaluator execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn islisp_profile_display() -> String {
    let profile = islisp_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn islisp_matrix_row_language_symbol_is_islisp() {
        let row = islisp_matrix_row();

        assert_eq!(row.language, Symbol::new("islisp"));
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
