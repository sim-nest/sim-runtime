//! Common Lisp conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

use crate::{cl_lite_profile, cl_lowering_symbol, cl_reader_symbol};

/// Builds the Common Lisp lite matrix row.
pub fn cl_lite_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("common-lisp"), cl_lite_profile())
        .with_cases(cl_lite_source_cases())
        .build()
}

/// Minimal source cases for the Common Lisp lite matrix row.
pub fn cl_lite_source_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/common-lisp-lite", "profile-declared"),
            organ: cl_reader_symbol(),
            source_name: "profile.sim".to_owned(),
            source: "profile".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::LowersTo(cl_lite_profile_display()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/common-lisp-lite", "runtime-gap"),
            organ: cl_lowering_symbol(),
            source_name: "runtime-gap.lisp".to_owned(),
            source: "(eval '(+ 1 2))".to_owned(),
            kind: SourceConformanceCaseKind::DescriptorOnly,
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("common-lisp", "runtime-gap"),
                reason: "CL-lite full runtime execution is outside this row".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

fn cl_lite_profile_display() -> String {
    let profile = cl_lite_profile();
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cl_lite_matrix_row_language_symbol_is_common_lisp() {
        let row = cl_lite_matrix_row();

        assert_eq!(row.language, Symbol::new("common-lisp"));
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
