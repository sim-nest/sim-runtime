use std::sync::Arc;

use sim_kernel::{
    ClaimKind, ClaimPattern, Cx, Datum, DatumStore, DefaultFactory, Expr, NoopEvalPolicy, Ref,
    Symbol,
};

use crate::{
    ConformanceMatrix, ExprRoundTripCase, ExprRoundTripObservation, LanguageProfile, LanguageRow,
    LanguageRowBuilder, MatrixCellKind, MatrixRunner, SourceConformanceCase,
    SourceConformanceCaseKind, SourceExpectation, SourceObservation, standard_test_capability,
    standard_test_case_predicate, standard_test_result_predicate,
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

    let report = MatrixRunner::run_source_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.cells.len(), 2);
    assert_eq!(report.pass_count(), 1);
    assert_eq!(report.gap_count(), 1);
    assert_eq!(report.fail_count(), 0);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), Some(1.0));
}

#[test]
fn matrix_runner_reports_expr_cells_and_descriptor_cells() {
    let mut cx = test_cx();
    let mixed_row = row_with_pass_gap_and_expr_cases();
    let descriptor_row = row_with_descriptor_cases();

    let mixed_report = MatrixRunner::run_row(
        &mut cx,
        &mixed_row,
        observation_for_case,
        expr_observation_for_case,
    );
    let descriptor_report =
        MatrixRunner::run_source_row(&mut cx, &descriptor_row, observation_for_case);

    assert_eq!(mixed_report.cells.len(), 4);
    assert_eq!(mixed_report.pass_count(), 2);
    assert_eq!(mixed_report.gap_count(), 2);
    assert_eq!(mixed_report.fail_count(), 0);
    assert!(
        mixed_report.cells.iter().any(|cell| cell.case_symbol
            == expr_case_symbol("expr-bool-true")
            && cell.kind == MatrixCellKind::ExprRoundTrip),
        "expected expression round-trip cells in the mixed report",
    );

    assert_eq!(descriptor_report.cells.len(), 2);
    assert_eq!(descriptor_report.pass_count(), 0);
    assert_eq!(descriptor_report.gap_count(), 0);
    assert_eq!(descriptor_report.fail_count(), 0);
    assert_eq!(
        descriptor_report.language_fidelity(&Symbol::new("scheme")),
        None
    );
}

#[test]
fn matrix_runner_publishes_cell_claims_with_kind_and_badge() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let row = row_with_pass_gap_and_expr_cases();
    let report = MatrixRunner::run_row(
        &mut cx,
        &row,
        observation_for_case,
        expr_observation_for_case,
    );

    report.publish_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(profile_result_claims(&row.profile.symbol))
        .unwrap();
    assert_eq!(claims.len(), 4);
    assert!(
        claims
            .iter()
            .any(|claim| has_case_claim(&cx, &claim.object, pass_case_symbol())),
        "expected a published evidence claim for the passing case"
    );
    let expr_evidence = claims
        .iter()
        .find(|claim| has_case_claim(&cx, &claim.object, expr_case_symbol("expr-bool-true")))
        .map(|claim| claim.object.clone())
        .expect("expected published evidence for expression case");
    let Ref::Content(evidence_id) = expr_evidence else {
        panic!("expected content-backed evidence");
    };
    let Some(Datum::Node { fields, .. }) = cx.datum_store().get(&evidence_id).unwrap() else {
        panic!("expected evidence datum node");
    };
    assert_eq!(
        node_field(fields, "cell-kind"),
        Some(&Datum::Symbol(Symbol::qualified(
            "standard-test",
            "expr-round-trip"
        )))
    );
    assert_eq!(
        node_field(fields, "affects-badge"),
        Some(&Datum::Symbol(Symbol::qualified("standard", "partial")))
    );
    assert!(claims.iter().all(|claim| claim.kind == ClaimKind::Observed));
}

#[test]
fn matrix_runner_fidelity_is_one_for_all_pass_row() {
    let mut cx = test_cx();
    let row = row_with_cases("scheme", 2);

    let report = MatrixRunner::run_source_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.pass_count(), 2);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), Some(1.0));
}

#[test]
fn matrix_runner_fidelity_is_none_for_gap_only_row() {
    let mut cx = test_cx();
    let row = row_with_gap_case();

    let report = MatrixRunner::run_source_row(&mut cx, &row, observation_for_case);

    assert_eq!(report.gap_count(), 1);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), None);
}

#[test]
fn conformance_card_fields_one_pass_one_gap_fidelity_is_100_percent() {
    let mut cx = test_cx();
    let row = row_with_pass_and_gap_cases();
    let report = MatrixRunner::run_source_row(&mut cx, &row, observation_for_case);

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
        kind: SourceConformanceCaseKind::Observed,
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

fn row_with_pass_gap_and_expr_cases() -> LanguageRow {
    LanguageRowBuilder::new(
        Symbol::new("scheme"),
        LanguageProfile::new(Symbol::qualified("lang", "scheme/v1")),
    )
    .with_case(pass_case())
    .with_case(gap_case())
    .with_expr_cases(vec![
        expr_case("expr-bool-true", Some("Expr::Bool(true)")),
        expr_case("expr-callcc-gap", None),
    ])
    .build()
}

fn row_with_descriptor_cases() -> LanguageRow {
    LanguageRowBuilder::new(
        Symbol::new("scheme"),
        LanguageProfile::new(Symbol::qualified("lang", "scheme/v1")),
    )
    .with_case(descriptor_pass_case())
    .with_case(descriptor_gap_case())
    .build()
}

fn pass_case() -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: pass_case_symbol(),
        organ: Symbol::qualified("scheme", "reader"),
        source_name: "pass.scm".to_owned(),
        source: "'answer".to_owned(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::LowersTo("expr".to_owned()),
        affects_badge: Some(Symbol::qualified("standard", "partial")),
    }
}

fn gap_case() -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: Symbol::qualified("test/matrix", "gap"),
        organ: Symbol::qualified("scheme", "lowering"),
        source_name: "gap.scm".to_owned(),
        source: "(eval '(+ 1 2))".to_owned(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::ExpectedGap {
            code: gap_code(),
            reason: "declared gap".to_owned(),
        },
        affects_badge: None,
    }
}

fn descriptor_pass_case() -> SourceConformanceCase {
    SourceConformanceCase {
        kind: SourceConformanceCaseKind::DescriptorOnly,
        ..pass_case()
    }
}

fn descriptor_gap_case() -> SourceConformanceCase {
    SourceConformanceCase {
        kind: SourceConformanceCaseKind::DescriptorOnly,
        ..gap_case()
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

fn expr_observation_for_case(
    _cx: &mut Cx,
    case: &ExprRoundTripCase,
) -> sim_kernel::Result<ExprRoundTripObservation> {
    match &case.expected_display {
        Some(expected) => Ok(ExprRoundTripObservation::RoundTripped(expected.clone())),
        None => Ok(ExprRoundTripObservation::Gap(Symbol::qualified(
            "codec",
            "declared-gap",
        ))),
    }
}

fn expr_case(name: &str, expected_display: Option<&str>) -> ExprRoundTripCase {
    ExprRoundTripCase {
        symbol: expr_case_symbol(name),
        language: Symbol::new("scheme"),
        source: "#t".to_owned(),
        expected_display: expected_display.map(str::to_owned),
        affects_badge: Some(Symbol::qualified("standard", "partial")),
    }
}

fn expr_case_symbol(name: &str) -> Symbol {
    Symbol::qualified("test/r7rs-small", name)
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

fn node_field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find_map(|(field, value)| (field == &Symbol::new(name)).then_some(value))
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}
