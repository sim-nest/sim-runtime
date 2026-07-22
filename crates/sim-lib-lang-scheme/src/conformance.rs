//! Scheme matrix-row conformance runner.

use sim_codec::{DecodeLimits, Input, ReadCx};
use sim_kernel::{CodecId, Cx, Datum, ReadPolicy, Result, Severity, Symbol, Term};
use sim_lib_standard_core::{
    ExprRoundTripCase, ExprRoundTripObservation, MatrixRunReport, MatrixRunner,
    SourceConformanceCase, SourceExpectation, SourceObservation,
};

use crate::{
    SchemeLowered, decode_scheme_tree, diagnose_unsupported_forms, r7rs_small_matrix_row,
    run_r7rs_small_restricted,
};

/// Runs one Scheme source conformance case through the R7RS-small restricted
/// runner.
pub fn run_r7rs_small_conformance_case(
    cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
    match run_r7rs_small_restricted(cx, &case.source) {
        Ok(lowered) => Ok(SourceObservation::LowersTo(lowered_display(&lowered))),
        Err(err)
            if matches!(case.expectation, SourceExpectation::ExpectedGap { .. })
                && err.to_string().contains("unsupported R7RS-small form") =>
        {
            Ok(SourceObservation::Gap {
                code: Symbol::qualified("scheme", "unsupported-form"),
                reason: err.to_string(),
            })
        }
        Err(err) => Err(err),
    }
}

fn run_r7rs_small_expr_case(
    cx: &mut Cx,
    case: &ExprRoundTripCase,
) -> Result<ExprRoundTripObservation> {
    Ok(case.run(cx, |cx, source| {
        let mut read_cx = ReadCx {
            cx,
            codec: CodecId(0),
            read_policy: ReadPolicy::default(),
            limits: DecodeLimits::default(),
        };
        let expr = decode_scheme_tree(
            &mut read_cx,
            "scheme-matrix-expr",
            Input::Text(source.to_owned()),
        )?
        .expr;
        if diagnose_unsupported_forms(&expr)
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            Ok(None)
        } else {
            Ok(Some(expr))
        }
    }))
}

/// Runs the Scheme matrix row and publishes one claim-backed evidence record for
/// each source case.
pub fn run_scheme_matrix_row(cx: &mut Cx) -> Result<MatrixRunReport> {
    let row = r7rs_small_matrix_row();
    let report = MatrixRunner::run_row(
        cx,
        &row,
        run_r7rs_small_conformance_case,
        run_r7rs_small_expr_case,
    );
    report.publish_claims(cx)?;
    Ok(report)
}

fn lowered_display(lowered: &SchemeLowered) -> String {
    match lowered {
        SchemeLowered::Datum(datum) => datum_display(datum),
        SchemeLowered::Term(term) => term_display(term),
    }
}

fn datum_display(datum: &Datum) -> String {
    match datum {
        Datum::Nil => "datum:nil".to_owned(),
        Datum::Bool(value) => format!("datum:bool {value}"),
        Datum::Number(value) => format!("datum:number {} {}", value.domain, value.canonical),
        Datum::Symbol(symbol) => format!("datum:symbol {symbol}"),
        Datum::String(value) => format!("datum:string {value}"),
        Datum::Bytes(value) => format!("datum:bytes {}", value.len()),
        Datum::List(items) => format!("datum:list {}", items.len()),
        Datum::Vector(items) => format!("datum:vector {}", items.len()),
        Datum::Map(entries) => format!("datum:map {}", entries.len()),
        Datum::Set(items) => format!("datum:set {}", items.len()),
        Datum::Node { tag, .. } => format!("datum:node {tag}"),
    }
}

fn term_display(term: &Term) -> String {
    format!("term:{term:?}")
}
