//! Publication helpers for generated coverage evidence.

use sim_kernel::{Cx, Result, Symbol, Value};
use sim_lib_standard_core::{
    ConformanceOutcome, MatrixCellKind, MatrixCellResult, MatrixRunReport,
};

use crate::{CoverageVerdict, GeneratedCoverageReport};

/// Builds browseable Card fields for one generated coverage report.
///
/// The fields use the `coverage.generated.*` namespace so they sit beside, and
/// never replace, curated `conformance.*` fields.
pub fn coverage_card_fields(
    cx: &mut Cx,
    report: &GeneratedCoverageReport,
    verdict: &CoverageVerdict,
) -> Result<Vec<(Symbol, Value)>> {
    let percent = match verdict {
        CoverageVerdict::Anchored {
            coverage_percent, ..
        } => format!("{coverage_percent:.0}%"),
        CoverageVerdict::Unanchored { .. } => "unanchored".to_owned(),
    };
    let citation = match verdict {
        CoverageVerdict::Anchored { citation, .. }
        | CoverageVerdict::Unanchored { citation, .. } => citation.to_string(),
    };
    Ok(vec![
        (
            coverage_generated_field("sampled"),
            count_value(cx, report.sampled)?,
        ),
        (
            coverage_generated_field("round-trip"),
            count_value(cx, report.round_tripped)?,
        ),
        (
            coverage_generated_field("percent"),
            cx.factory().string(percent)?,
        ),
        (
            coverage_generated_field("citation"),
            cx.factory().string(citation)?,
        ),
    ])
}

/// Publishes one standard test-run evidence claim for a generated coverage run.
///
/// The evidence uses the existing matrix claim publication path and standard
/// test predicates. Anchored reports publish as passing evidence; unanchored
/// reports publish as declared gaps with the anchor reason in the detail.
pub fn publish_coverage_claims(
    cx: &mut Cx,
    report: &GeneratedCoverageReport,
    verdict: &CoverageVerdict,
) -> Result<()> {
    let outcome = match verdict {
        CoverageVerdict::Anchored { .. } => ConformanceOutcome::pass(),
        CoverageVerdict::Unanchored { reason, .. } => ConformanceOutcome::gap(reason.clone()),
    };
    MatrixRunReport {
        cells: vec![MatrixCellResult {
            language: report.language.clone(),
            profile: generated_coverage_profile_symbol(&report.language),
            organ: generated_coverage_organ_symbol(),
            case_symbol: generated_coverage_case_symbol(&report.language),
            kind: MatrixCellKind::GeneratedCoverage,
            affects_badge: None,
            outcome,
        }],
    }
    .publish_claims(cx)
}

/// Profile symbol used as the subject of generated coverage evidence claims.
pub fn generated_coverage_profile_symbol(language: &Symbol) -> Symbol {
    Symbol::qualified(
        "lang/generated",
        format!("{}-coverage", language.as_qualified_str()),
    )
}

fn coverage_generated_field(name: &str) -> Symbol {
    Symbol::new(format!("coverage.generated.{name}"))
}

fn generated_coverage_organ_symbol() -> Symbol {
    Symbol::qualified("coverage", "generated")
}

fn generated_coverage_case_symbol(language: &Symbol) -> Symbol {
    Symbol::qualified(
        "coverage/generated",
        format!("{}-run", language.as_qualified_str()),
    )
}

fn count_value(cx: &mut Cx, count: usize) -> Result<Value> {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "u64"), count.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{
        ClaimKind, ClaimPattern, Cx, Datum, DatumStore, DefaultFactory, Expr, NoopEvalPolicy, Ref,
        Symbol,
    };
    use sim_lib_standard_core::{
        MatrixRunReport, standard_test_capability, standard_test_case_predicate,
        standard_test_result_predicate,
    };

    use super::*;

    #[test]
    fn coverage_fields_do_not_touch_curated_fidelity() {
        let mut cx = test_cx();
        let report = anchored_report();
        let verdict = anchored_verdict();
        let generated = coverage_card_fields(&mut cx, &report, &verdict).unwrap();
        let curated = MatrixRunReport::unscored_conformance_card_fields(&mut cx).unwrap();

        assert_eq!(
            string_field(&mut cx, &generated, "coverage.generated.percent"),
            "75%"
        );
        assert_eq!(
            string_field(&mut cx, &curated, "conformance.fidelity"),
            "unscored"
        );
        assert!(missing_field(&generated, "conformance.fidelity"));
        assert!(missing_field(&curated, "coverage.generated.percent"));
    }

    #[test]
    fn unanchored_coverage_field_suppresses_percent() {
        let mut cx = test_cx();
        let report = anchored_report();
        let verdict = CoverageVerdict::Unanchored {
            citation: Symbol::new("r7rs-small"),
            reason: "curated landmark not reproduced".to_owned(),
        };

        let fields = coverage_card_fields(&mut cx, &report, &verdict).unwrap();

        assert_eq!(
            string_field(&mut cx, &fields, "coverage.generated.percent"),
            "unanchored"
        );
        assert_eq!(
            string_field(&mut cx, &fields, "coverage.generated.citation"),
            "r7rs-small"
        );
        assert_eq!(
            number_field(&mut cx, &fields, "coverage.generated.sampled"),
            "4"
        );
        assert_eq!(
            number_field(&mut cx, &fields, "coverage.generated.round-trip"),
            "3"
        );
    }

    #[test]
    fn coverage_claims_use_standard_test_run_evidence() {
        let mut cx = test_cx();
        cx.grant(standard_test_capability());
        let report = anchored_report();
        let verdict = anchored_verdict();

        publish_coverage_claims(&mut cx, &report, &verdict).unwrap();

        let claims = cx
            .query_facts(profile_result_claims(&generated_coverage_profile_symbol(
                &report.language,
            )))
            .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].kind, ClaimKind::Observed);
        assert!(has_case_claim(
            &cx,
            &claims[0].object,
            generated_coverage_case_symbol(&report.language)
        ));
        let Ref::Content(evidence_id) = claims[0].object.clone() else {
            panic!("expected content-backed evidence");
        };
        let Some(Datum::Node { fields, .. }) = cx.datum_store().get(&evidence_id).unwrap() else {
            panic!("expected evidence datum node");
        };
        assert_eq!(
            node_field(fields, "cell-kind"),
            Some(&Datum::Symbol(Symbol::qualified(
                "standard-test",
                "generated-coverage",
            )))
        );
    }

    fn anchored_report() -> GeneratedCoverageReport {
        GeneratedCoverageReport {
            language: Symbol::new("scheme"),
            matrix_report: MatrixRunReport { cells: Vec::new() },
            sampled: 4,
            round_tripped: 3,
            mismatched: 1,
            diagnostics: 0,
            max_depth: 2,
            seed: vec![Expr::Bool(true)],
            landmark_reproduced: true,
            unmet_landmarks: Vec::new(),
        }
    }

    fn anchored_verdict() -> CoverageVerdict {
        CoverageVerdict::Anchored {
            citation: Symbol::new("r7rs-small"),
            coverage_percent: 75.0,
            curated_fidelity_level: 1,
            claim_level: 1,
        }
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

    fn missing_field(fields: &[(Symbol, Value)], name: &str) -> bool {
        fields.iter().all(|(field, _)| field != &Symbol::new(name))
    }

    fn number_field(cx: &mut Cx, fields: &[(Symbol, Value)], name: &str) -> String {
        let Expr::Number(number) = field_expr(cx, fields, name) else {
            panic!("expected number field {name}");
        };
        assert_eq!(number.domain, Symbol::qualified("numbers", "u64"));
        number.canonical
    }

    fn string_field(cx: &mut Cx, fields: &[(Symbol, Value)], name: &str) -> String {
        let Expr::String(value) = field_expr(cx, fields, name) else {
            panic!("expected string field {name}");
        };
        value
    }

    fn field_expr(cx: &mut Cx, fields: &[(Symbol, Value)], name: &str) -> Expr {
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
}
