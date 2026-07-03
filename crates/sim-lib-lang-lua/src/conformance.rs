//! Lua matrix-row conformance runner.

use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, MatrixRunReport, MatrixRunner, SourceConformanceCase, SourceExpectation,
    SourceObservation,
};

use crate::{lua_core_matrix_row, lua_core_profile};

/// Runs one Lua core source conformance case.
pub fn run_lua_core_conformance_case(
    _cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
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
    let report = MatrixRunner::run_row(cx, &row, run_lua_core_conformance_case);
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

#[cfg(test)]
mod tests {
    use sim_kernel::testing::bare_cx as cx;
    use sim_lib_standard_core::standard_test_capability;

    use super::*;

    #[test]
    fn lua_core_matrix_row_runner_reports_profile_pass_and_runtime_gap() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let report = run_lua_core_matrix_row(&mut cx).unwrap();

        assert_eq!(report.cells.len(), 2);
        assert_eq!(report.pass_count(), 1);
        assert_eq!(report.gap_count(), 1);
        assert_eq!(report.fail_count(), 0);
    }
}
