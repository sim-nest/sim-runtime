//! Scheme conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    ExprRoundTripCase, LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceExpectation,
};

use crate::{r7rs_small_profile, scheme_lowering_symbol, scheme_reader_symbol};

/// Builds the Scheme R7RS-small matrix row.
pub fn r7rs_small_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("scheme"), r7rs_small_profile())
        .with_cases(r7rs_small_stub_cases())
        .with_expr_cases(r7rs_small_expr_cases())
        .build()
}

/// Minimal R7RS-small source cases for the matrix.
pub fn r7rs_small_stub_cases() -> Vec<SourceConformanceCase> {
    vec![
        SourceConformanceCase {
            symbol: Symbol::qualified("test/r7rs-small", "quote-symbol"),
            organ: scheme_reader_symbol(),
            source_name: "quote-symbol.scm".to_owned(),
            source: "'answer".to_owned(),
            expectation: SourceExpectation::LowersTo("datum:symbol answer".to_owned()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("test/r7rs-small", "eval-gap"),
            organ: scheme_lowering_symbol(),
            source_name: "eval-gap.scm".to_owned(),
            source: "(eval '(+ 1 2))".to_owned(),
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("scheme", "unsupported-form"),
                reason: "read-eval is capability gated".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

/// R7RS-small expression round-trip cases for the Scheme row.
pub fn r7rs_small_expr_cases() -> Vec<ExprRoundTripCase> {
    vec![
        ExprRoundTripCase {
            symbol: Symbol::qualified("test/r7rs-small", "expr-bool-true"),
            language: Symbol::new("scheme"),
            source: "#t".to_owned(),
            expected_display: Some("Expr::Bool(true)".to_owned()),
            affects_badge: Some(Symbol::qualified("standard", "partial")),
        },
        ExprRoundTripCase {
            symbol: Symbol::qualified("test/r7rs-small", "expr-callcc-gap"),
            language: Symbol::new("scheme"),
            source: "(call/cc (lambda (k) k))".to_owned(),
            expected_display: None,
            affects_badge: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r7rs_small_matrix_row_language_is_scheme_and_has_cases() {
        let row = r7rs_small_matrix_row();

        assert_eq!(row.language, Symbol::new("scheme"));
        assert!(!row.is_empty());
        assert_eq!(row.cases.len(), 2);
        assert_eq!(row.expr_cases.len(), 2);
    }

    #[test]
    fn r7rs_small_expr_cases_count_is_two() {
        let cases = r7rs_small_expr_cases();

        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].source, "#t");
        assert_eq!(
            cases[0].expected_display.as_deref(),
            Some("Expr::Bool(true)")
        );
        assert_eq!(cases[1].source, "(call/cc (lambda (k) k))");
        assert_eq!(cases[1].expected_display, None);
    }
}
