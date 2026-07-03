//! Scheme matrix-row conformance runner.

use sim_kernel::{Cx, Datum, Result, Term};
use sim_lib_standard_core::{
    MatrixRunReport, MatrixRunner, SourceConformanceCase, SourceExpectation, SourceObservation,
};

use crate::{SchemeLowered, r7rs_small_matrix_row, run_r7rs_small_restricted};

/// Runs one Scheme source conformance case through the R7RS-small restricted
/// runner.
pub fn run_r7rs_small_conformance_case(
    cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
    if let SourceExpectation::ExpectedGap { code, reason } = &case.expectation {
        return Ok(SourceObservation::Gap {
            code: code.clone(),
            reason: reason.clone(),
        });
    }
    run_r7rs_small_restricted(cx, &case.source)
        .map(|lowered| SourceObservation::LowersTo(lowered_display(&lowered)))
}

/// Runs the Scheme matrix row and publishes one claim-backed evidence record for
/// each source case.
pub fn run_scheme_matrix_row(cx: &mut Cx) -> Result<MatrixRunReport> {
    let row = r7rs_small_matrix_row();
    let report = MatrixRunner::run_row(cx, &row, run_r7rs_small_conformance_case);
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
