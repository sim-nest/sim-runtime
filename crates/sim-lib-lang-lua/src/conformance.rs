//! Lua matrix-row conformance runner.

use sim_kernel::{Cx, Expr, Result, Symbol, Value};
use sim_lib_standard_core::{
    LanguageProfile, MatrixRunReport, MatrixRunner, SourceConformanceCase, SourceExpectation,
    SourceObservation,
};

use crate::{load::eval_lua_source, lua_core_matrix_row, lua_core_profile, lua_rawget};

/// One row in the Lua shared-substrate reuse ledger.
///
/// The ledger is deliberately small and concrete: each entry names the shared
/// runtime extension Lua uses, a passing non-Lua or substrate test that proves
/// the extension is not Lua-only, and sibling language scaffolds that can adopt
/// the same extension without forking it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReuseLedgerEntry {
    /// Shared extension reused by the Lua runtime.
    pub shared_extension: &'static str,
    /// Passing non-Lua or substrate test that backs the shared claim.
    pub passing_non_lua_test: &'static str,
    /// Sibling scaffolds that can adopt the same extension.
    pub sibling_scaffolds: &'static [&'static str],
}

/// Lua reuse ledger for the shared language-runtime substrate.
pub const REUSE_LEDGER: &[ReuseLedgerEntry] = &[
    ReuseLedgerEntry {
        shared_extension: "GuestRuntimeKit",
        passing_non_lua_test: "sim-lib-standard-core::guest_kit_tests::guest_runtime_kits_cover_distinct_truth_and_arity_rules",
        sibling_scaffolds: &["Ruby blocks", "Julia calls", "typed-lazy forcing"],
    },
    ReuseLedgerEntry {
        shared_extension: "BindingCell",
        passing_non_lua_test: "sim-lib-binding::tests::captured_binding_cell_is_shared_by_two_closures",
        sibling_scaffolds: &["Scheme closures", "Common Lisp lexical functions"],
    },
    ReuseLedgerEntry {
        shared_extension: "RuntimeKey/MutableRuntimeTable",
        passing_non_lua_test: "sim-lib-mutation::tests::runtime_table_accepts_dict_keys_and_projects_array",
        sibling_scaffolds: &["Ruby hash", "Clojure map literals"],
    },
    ReuseLedgerEntry {
        shared_extension: "MetaObjectProtocol",
        passing_non_lua_test: "sim-lib-dispatch::tests::meta_index_walks_prototype_chain_through_protocol_override",
        sibling_scaffolds: &["Ruby method lookup", "Julia property access"],
    },
    ReuseLedgerEntry {
        shared_extension: "protected_call/coroutine/close",
        passing_non_lua_test: "sim-lib-control::frame_tests::coroutine_frame_produces_and_consumes_without_surface_names",
        sibling_scaffolds: &["Ruby ensure blocks", "Scheme continuations"],
    },
    ReuseLedgerEntry {
        shared_extension: "text-pattern VM",
        passing_non_lua_test: "sim-lib-pattern::text_tests::lua_dialect_preserves_captures_and_budget_limits",
        sibling_scaffolds: &["glob codec", "Ruby regexp facade"],
    },
];

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
    if matches!(case.expectation, SourceExpectation::ExpectedGap { .. }) {
        return run_lua_expected_gap_case(cx, case);
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

fn run_lua_expected_gap_case(
    cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
    let SourceExpectation::ExpectedGap { .. } = &case.expectation else {
        unreachable!("expected gap runner called for non-gap case");
    };
    let values = eval_lua_source(cx, &case.source)?;
    if let Some(value) = values.first()
        && let Some((code, reason)) = expected_gap_value(cx, value)?
    {
        return Ok(SourceObservation::Gap { code, reason });
    }
    Ok(SourceObservation::LowersTo(values_display(cx, &values)?))
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

fn expected_gap_value(cx: &mut Cx, value: &Value) -> Result<Option<(Symbol, String)>> {
    if table_string_field(cx, value, "kind")?.as_deref() != Some("ExpectedGap") {
        return Ok(None);
    }
    let code = table_string_field(cx, value, "code")?
        .unwrap_or_else(|| "lua.unknown-expected-gap".to_owned());
    let reason = table_string_field(cx, value, "reason")?.unwrap_or_default();
    Ok(Some((Symbol::new(code), reason)))
}

fn table_string_field(cx: &mut Cx, value: &Value, field: &str) -> Result<Option<String>> {
    let key = cx.factory().string(field.to_owned())?;
    let Some(value) = lua_rawget(cx, value, &key)? else {
        return Ok(None);
    };
    match value.object().as_expr(cx)? {
        Expr::Nil => Ok(None),
        Expr::String(text) => Ok(Some(text)),
        _ => value.object().display(cx).map(Some),
    }
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
        cx.grant(sim_lib_mutation::standard_mutate_capability());

        let report = run_lua_core_matrix_row(&mut cx).unwrap();

        assert_eq!(report.cells.len(), 9);
        assert_eq!(report.pass_count(), 5, "{:#?}", report.cells);
        assert_eq!(report.gap_count(), 3, "{:#?}", report.cells);
        assert_eq!(report.fail_count(), 0, "{:#?}", report.cells);
        assert_eq!(report.language_fidelity(&Symbol::new("lua")), Some(1.0));
    }

    #[test]
    fn lua_reuse_ledger_names_shared_substrate_and_adopters() {
        assert_eq!(REUSE_LEDGER.len(), 6);
        assert!(REUSE_LEDGER.iter().any(|entry| {
            entry.shared_extension == "RuntimeKey/MutableRuntimeTable"
                && entry
                    .sibling_scaffolds
                    .iter()
                    .any(|name| name.contains("Ruby"))
        }));
        assert!(REUSE_LEDGER.iter().all(|entry| {
            !entry.passing_non_lua_test.is_empty() && !entry.sibling_scaffolds.is_empty()
        }));
    }
}
