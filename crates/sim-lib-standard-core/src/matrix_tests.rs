use std::sync::Arc;

use sim_kernel::Symbol;
use sim_kernel::{ClaimKind, ClaimPattern, Cx, DefaultFactory, Expr, NoopEvalPolicy, Ref};

use crate::{
    ConformanceMatrix, ExprRoundTripCase, ExprRoundTripObservation, LanguageProfile, LanguageRow,
    LanguageRowBuilder, MatrixRunner, SourceConformanceCase, SourceExpectation, SourceObservation,
    standard_test_capability, standard_test_case_predicate, standard_test_result_predicate,
};

#[test]
fn conformance_matrix_registers_and_counts_rows() {
    let mut matrix = ConformanceMatrix::new();
    matrix.register(row_with_cases("scheme", 2));
    matrix.register(LanguageRow::declared_empty(
        Symbol::new("lua"),
        LanguageProfile::new(Symbol::qualified("lang", "lua-core/v1")),
    ));

    assert_eq!(matrix.language_count(), 2);
    assert_eq!(matrix.total_cases(), 2);
    assert_eq!(
        matrix.row(&Symbol::new("scheme")).unwrap().language,
        Symbol::new("scheme")
    );
    let languages: Vec<_> = matrix
        .iter_rows()
        .map(|row| row.language.to_string())
        .collect();
    assert_eq!(languages, vec!["scheme", "lua"]);
}

#[test]
fn conformance_matrix_declared_empty_row_is_empty() {
    let row = LanguageRow::declared_empty(
        Symbol::new("ruby"),
        LanguageProfile::new(Symbol::qualified("lang", "ruby-dsl/v1")),
    );

    assert!(row.is_empty());
}

#[test]
fn expr_round_trip_cases_attach_to_language_row() {
    let row = row_with_cases("scheme", 1)
        .with_expr_cases(vec![expr_case("expr-bool-true", Some("Expr::Bool(true)"))]);

    assert!(!row.is_empty());
    assert_eq!(row.cases.len(), 1);
    assert_eq!(row.expr_cases.len(), 1);
}

#[test]
fn expr_round_trip_pass_returns_round_tripped() {
    let mut cx = test_cx();
    let case = expr_case("expr-bool-true", Some("Expr::Bool(true)"));

    let observation = case.run_expr_round_trip(&mut cx, |_cx, _source| Ok(Some(Expr::Bool(true))));

    assert_eq!(
        observation,
        ExprRoundTripObservation::RoundTripped("Expr::Bool(true)".to_owned())
    );
}

#[test]
fn expr_round_trip_mismatch_returns_mismatch() {
    let mut cx = test_cx();
    let case = expr_case("expr-bool-true", Some("Expr::Bool(false)"));

    let observation = case.run_expr_round_trip(&mut cx, |_cx, _source| Ok(Some(Expr::Bool(true))));

    assert_eq!(
        observation,
        ExprRoundTripObservation::Mismatch {
            expected: "Expr::Bool(false)".to_owned(),
            got: "Expr::Bool(true)".to_owned(),
        }
    );
}

#[test]
fn expr_round_trip_gap_returns_gap() {
    let mut cx = test_cx();
    let case = expr_case("expr-callcc-gap", None);

    let observation = case.run_expr_round_trip(&mut cx, |_cx, _source| Ok(None));

    assert_eq!(
        observation,
        ExprRoundTripObservation::Gap(Symbol::qualified("codec", "declared-gap"))
    );
}

#[test]
#[should_panic(expected = "language already registered in matrix")]
fn conformance_matrix_rejects_duplicate_language() {
    let mut matrix = ConformanceMatrix::new();
    matrix.register(row_with_cases("scheme", 1));
    matrix.register(row_with_cases("scheme", 1));
}

#[test]
fn matrix_runner_single_pass_row_report_is_correct() {
    let mut cx = test_cx();
    let row = row_with_pass_and_gap_cases();

    let report = MatrixRunner::run_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.cells.len(), 2);
    assert_eq!(report.pass_count(), 1);
    assert_eq!(report.gap_count(), 1);
    assert_eq!(report.fail_count(), 0);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), Some(1.0));
}

#[test]
fn matrix_runner_publishes_cell_claims() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let row = row_with_pass_and_gap_cases();
    let report = MatrixRunner::run_row(&mut cx, &row, observation_for_case);

    report.publish_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(profile_result_claims(&row.profile.symbol))
        .unwrap();
    assert_eq!(claims.len(), 2);
    assert!(
        claims
            .iter()
            .any(|claim| has_case_claim(&cx, &claim.object, pass_case_symbol())),
        "expected a published evidence claim for the passing case"
    );
    assert!(claims.iter().all(|claim| claim.kind == ClaimKind::Observed));
}

#[test]
fn matrix_runner_fidelity_is_one_for_all_pass_row() {
    let mut cx = test_cx();
    let row = row_with_cases("scheme", 2);

    let report = MatrixRunner::run_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.pass_count(), 2);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), Some(1.0));
}

#[test]
fn matrix_runner_fidelity_is_none_for_gap_only_row() {
    let mut cx = test_cx();
    let row = row_with_gap_case();

    let report = MatrixRunner::run_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.gap_count(), 1);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), None);
}

#[test]
fn conformance_card_fields_one_pass_one_gap_fidelity_is_100_percent() {
    let mut cx = test_cx();
    let row = row_with_pass_and_gap_cases();
    let report = MatrixRunner::run_row(&mut cx, &row, observation_for_case);

    let fields = report
        .conformance_card_fields(&mut cx, &Symbol::new("scheme"))
        .unwrap();

    assert_eq!(number_field(&mut cx, &fields, "conformance.pass"), "1");
    assert_eq!(number_field(&mut cx, &fields, "conformance.gap"), "1");
    assert_eq!(number_field(&mut cx, &fields, "conformance.fail"), "0");
    assert_eq!(
        string_field(&mut cx, &fields, "conformance.fidelity"),
        "100%"
    );
}

#[test]
fn conformance_card_fields_no_cells_fidelity_is_unscored() {
    let mut cx = test_cx();
    let report = crate::MatrixRunReport { cells: Vec::new() };

    let fields = report
        .conformance_card_fields(&mut cx, &Symbol::new("scheme"))
        .unwrap();

    assert_eq!(number_field(&mut cx, &fields, "conformance.pass"), "0");
    assert_eq!(number_field(&mut cx, &fields, "conformance.gap"), "0");
    assert_eq!(number_field(&mut cx, &fields, "conformance.fail"), "0");
    assert_eq!(
        string_field(&mut cx, &fields, "conformance.fidelity"),
        "unscored"
    );
}

fn row_with_cases(language: &str, case_count: usize) -> LanguageRow {
    let cases = (0..case_count).map(|index| SourceConformanceCase {
        symbol: Symbol::qualified("test/matrix", format!("{language}-{index}")),
        organ: Symbol::qualified(language, "reader"),
        source_name: format!("{language}-{index}.src"),
        source: "source".to_owned(),
        expectation: SourceExpectation::LowersTo("expr".to_owned()),
        affects_badge: None,
    });
    LanguageRowBuilder::new(
        Symbol::new(language),
        LanguageProfile::new(Symbol::qualified("lang", format!("{language}/v1"))),
    )
    .with_cases(cases)
    .build()
}

fn row_with_pass_and_gap_cases() -> LanguageRow {
    LanguageRowBuilder::new(
        Symbol::new("scheme"),
        LanguageProfile::new(Symbol::qualified("lang", "scheme/v1")),
    )
    .with_case(pass_case())
    .with_case(gap_case())
    .build()
}

fn row_with_gap_case() -> LanguageRow {
    LanguageRowBuilder::new(
        Symbol::new("scheme"),
        LanguageProfile::new(Symbol::qualified("lang", "scheme/v1")),
    )
    .with_case(gap_case())
    .build()
}

fn pass_case() -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: pass_case_symbol(),
        organ: Symbol::qualified("scheme", "reader"),
        source_name: "pass.scm".to_owned(),
        source: "'answer".to_owned(),
        expectation: SourceExpectation::LowersTo("expr".to_owned()),
        affects_badge: None,
    }
}

fn gap_case() -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: Symbol::qualified("test/matrix", "gap"),
        organ: Symbol::qualified("scheme", "lowering"),
        source_name: "gap.scm".to_owned(),
        source: "(eval '(+ 1 2))".to_owned(),
        expectation: SourceExpectation::ExpectedGap {
            code: gap_code(),
            reason: "declared gap".to_owned(),
        },
        affects_badge: None,
    }
}

fn observation_for_case(
    _cx: &mut Cx,
    case: &SourceConformanceCase,
) -> sim_kernel::Result<SourceObservation> {
    match &case.expectation {
        SourceExpectation::LowersTo(expected) => Ok(SourceObservation::LowersTo(expected.clone())),
        SourceExpectation::ExpectedGap { code, reason } => Ok(SourceObservation::Gap {
            code: code.clone(),
            reason: reason.clone(),
        }),
    }
}

fn expr_case(name: &str, expected_display: Option<&str>) -> ExprRoundTripCase {
    ExprRoundTripCase {
        symbol: Symbol::qualified("test/r7rs-small", name),
        language: Symbol::new("scheme"),
        source: "#t".to_owned(),
        expected_display: expected_display.map(str::to_owned),
        affects_badge: Some(Symbol::qualified("standard", "partial")),
    }
}

fn pass_case_symbol() -> Symbol {
    Symbol::qualified("test/matrix", "pass")
}

fn gap_code() -> Symbol {
    Symbol::qualified("test", "gap")
}

fn profile_result_claims(profile: &Symbol) -> ClaimPattern {
    ClaimPattern {
        subject: Some(Ref::Symbol(profile.clone())),
        predicate: Some(standard_test_result_predicate()),
        object: None,
        include_revoked: false,
    }
}

fn has_case_claim(cx: &Cx, evidence: &Ref, case: Symbol) -> bool {
    cx.query_facts(ClaimPattern::exact(
        evidence.clone(),
        standard_test_case_predicate(),
        Ref::Symbol(case),
    ))
    .map(|claims| !claims.is_empty())
    .unwrap_or(false)
}

fn number_field(cx: &mut Cx, fields: &[(Symbol, sim_kernel::Value)], name: &str) -> String {
    let Expr::Number(number) = field_expr(cx, fields, name) else {
        panic!("expected number field {name}");
    };
    assert_eq!(number.domain, Symbol::qualified("numbers", "u64"));
    number.canonical
}

fn string_field(cx: &mut Cx, fields: &[(Symbol, sim_kernel::Value)], name: &str) -> String {
    let Expr::String(value) = field_expr(cx, fields, name) else {
        panic!("expected string field {name}");
    };
    value
}

fn field_expr(cx: &mut Cx, fields: &[(Symbol, sim_kernel::Value)], name: &str) -> Expr {
    fields
        .iter()
        .find(|(field, _)| field == &Symbol::new(name))
        .unwrap_or_else(|| panic!("missing field {name}"))
        .1
        .object()
        .as_expr(cx)
        .unwrap()
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}
