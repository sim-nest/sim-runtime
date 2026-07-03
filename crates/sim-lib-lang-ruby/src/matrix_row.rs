//! Ruby conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceExpectation,
};

use crate::{ruby_dsl_profile, ruby_lowering_symbol, ruby_reader_symbol};

/// Builds the Ruby DSL matrix row.
pub fn ruby_dsl_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("ruby"), ruby_dsl_profile())
        .with_cases(ruby_dsl_source_cases())
        .build()
}

/// Minimal source cases for the Ruby DSL matrix row.
pub fn ruby_dsl_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/ruby-dsl", "profile-declared"),
            organ: ruby_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            expectation: SourceExpectation::LowersTo(ruby_dsl_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/ruby-dsl", "runtime-gap"),
            organ: ruby_lowering_symbol(),
            source_name: "runtime-gap.rb".to_owned(),
            source: "eval('1 + 2')".to_owned(),
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("ruby", "runtime-gap"),
                reason: "Ruby full object-model execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn ruby_dsl_profile_display() -> String {
    let profile = ruby_dsl_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_dsl_matrix_row_language_symbol_is_ruby() {
        let row = ruby_dsl_matrix_row();

        assert_eq!(row.language, Symbol::new("ruby"));
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
