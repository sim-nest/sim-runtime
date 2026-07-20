//! Lua matrix-row conformance runner.

use sim_kernel::{Cx, Expr, Result, Symbol, Value};
use sim_lib_standard_core::{
    LanguageProfile, MatrixRunReport, MatrixRunner, SourceConformanceCase, SourceExpectation,
    SourceObservation,
};

use crate::{load::eval_lua_source, lua_core_matrix_row, lua_core_profile};

/// Runs one Lua core source conformance case.
pub fn run_lua_core_conformance_case(
    cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
    if case.source == "profile" {
        return Ok(observe_profile_backed_case(
            case,
            &lua_core_profile(),
            Symbol::qualified("lua", "unsupported-source-case"),
            "case is outside Lua profile descriptor coverage",
        ));
    }
    if matches!(case.expectation, SourceExpectation::LowersTo(_)) {
        let values = eval_lua_source(cx, &case.source)?;
        return Ok(SourceObservation::LowersTo(values_display(cx, &values)?));
    }
    Ok(observe_profile_backed_case(
        case,
        &lua_core_profile(),
        Symbol::qualified("lua", "unsupported-source-case"),
        "case is outside Lua profile descriptor coverage",
    ))
}

/// Runs the Lua core matrix row and publishes claim-backed cells.
pub fn run_lua_core_matrix_row(cx: &mut Cx) -> Result<MatrixRunReport> {
    let row = lua_core_matrix_row();
    let report = MatrixRunner::run_source_row(cx, &row, run_lua_core_conformance_case);
    report.publish_claims(cx)?;
    Ok(report)
}

fn observe_profile_backed_case(
    case: &SourceConformanceCase,
    profile: &LanguageProfile,
    unsupported_code: Symbol,
    unsupported_reason: &str,
) -> SourceObservation {
    match &case.expectation {
        SourceExpectation::ExpectedGap { code, reason } => SourceObservation::Gap {
            code: code.clone(),
            reason: reason.clone(),
        },
        SourceExpectation::LowersTo(_) if case.source == "profile" => {
            SourceObservation::LowersTo(profile_display(profile))
        }
        SourceExpectation::LowersTo(_) => SourceObservation::Gap {
            code: unsupported_code,
            reason: unsupported_reason.to_owned(),
        },
    }
}

fn profile_display(profile: &LanguageProfile) -> String {
    format!(
        "profile:{} reader:{} lowering:{}",
        profile.symbol, profile.reader, profile.lowering
    )
}

fn values_display(cx: &mut Cx, values: &[Value]) -> Result<String> {
    if values.len() == 1 {
        return value_display(cx, &values[0]);
    }
    values
        .iter()
        .map(|value| value_display(cx, value))
        .collect::<Result<Vec<_>>>()
        .map(|values| values.join(", "))
}

fn value_display(cx: &mut Cx, value: &Value) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::Nil => Ok("nil".to_owned()),
        Expr::Bool(value) => Ok(value.to_string()),
        Expr::Number(number) => Ok(number.canonical),
        Expr::String(value) => Ok(value),
        other => Ok(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use sim_kernel::testing::bare_cx as cx;
    use sim_lib_standard_core::standard_test_capability;

    use super::*;

    #[test]
    fn lua_core_matrix_row_runner_reports_profile_pass_and_load_source_pass() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let report = run_lua_core_matrix_row(&mut cx).unwrap();

        assert_eq!(report.cells.len(), 2);
        assert_eq!(report.pass_count(), 1);
        assert_eq!(report.gap_count(), 0);
        assert_eq!(report.fail_count(), 0);
        assert_eq!(report.language_fidelity(&Symbol::new("lua")), Some(1.0));
    }
}
