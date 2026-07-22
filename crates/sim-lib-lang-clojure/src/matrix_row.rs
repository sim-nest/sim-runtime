//! Clojure conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

use crate::{clojure_core_profile, clojure_edn_reader_symbol, clojure_lowering_symbol};

/// Builds the Clojure core matrix row.
pub fn clojure_core_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("clojure"), clojure_core_profile())
        .with_cases(clojure_core_source_cases())
        .build()
}

/// Minimal source cases for the Clojure core matrix row.
pub fn clojure_core_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/clojure-core", "profile-declared"),
            organ: clojure_edn_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::LowersTo(clojure_core_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/clojure-core", "runtime-gap"),
            organ: clojure_lowering_symbol(),
            source_name: "runtime-gap.clj".to_owned(),
            source: "(eval '(+ 1 2))".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("clojure", "runtime-gap"),
                reason: "Clojure eval forms are outside the EDN profile row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn clojure_core_profile_display() -> String {
    let profile = clojure_core_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clojure_core_matrix_row_language_symbol_is_clojure() {
        let row = clojure_core_matrix_row();

        assert_eq!(row.language, Symbol::new("clojure"));
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
