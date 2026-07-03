//! Generated expression coverage for the Prolog language row.

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{Cx, Result, Symbol};
use sim_lib_lang_genconf::{
    CoverageVerdict, ExprSpace, GeneratedCoverageReport, LandmarkCorpus, coverage_card_fields,
    publish_coverage_claims, run_generated_row,
};
use sim_lib_standard_core::standard_test_capability;

use crate::prolog_reader_symbol;

const PROLOG_GENERATED_BUDGET: usize = 256;

/// Published generated expression coverage for the Prolog row.
pub struct PrologGeneratedCoverage {
    /// Measured generated round-trip report.
    pub report: GeneratedCoverageReport,
    /// Anchor verdict controlling public coverage fields and claims.
    pub verdict: CoverageVerdict,
}

/// Runs the Prolog row through the shared generated expression conformance path.
///
/// The generated claim is supplementary evidence under a generated-coverage
/// profile. It does not alter the curated Prolog conformance badge.
pub fn run_prolog_generated_coverage(cx: &mut Cx) -> Result<PrologGeneratedCoverage> {
    cx.require(&standard_test_capability())?;
    ensure_lisp_codec(cx)?;

    let language = prolog_language_symbol();
    let codec = prolog_reader_symbol();
    let space = ExprSpace::core_round_trip_space(3);
    let report = run_generated_row(cx, &language, &codec, &space, PROLOG_GENERATED_BUDGET);
    let verdict = prolog_generated_corpus(&space).reconcile(&report);
    publish_coverage_claims(cx, &report, &verdict)?;

    Ok(PrologGeneratedCoverage { report, verdict })
}

pub(crate) fn prolog_generated_coverage_card_fields(
    cx: &mut Cx,
) -> Result<Vec<(Symbol, sim_kernel::Value)>> {
    let coverage = run_prolog_generated_coverage(cx)?;
    coverage_card_fields(cx, &coverage.report, &coverage.verdict)
}

fn ensure_lisp_codec(cx: &mut Cx) -> Result<()> {
    if cx.resolve_codec(&prolog_reader_symbol()).is_ok() {
        return Ok(());
    }
    let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id())?;
    cx.load_lib(&lib).map(|_| ())
}

fn prolog_generated_corpus(space: &ExprSpace) -> LandmarkCorpus {
    LandmarkCorpus::new(
        prolog_language_symbol(),
        Symbol::qualified("expr-space", "core"),
        1,
        space.seed_corpus(),
    )
}

fn prolog_language_symbol() -> Symbol {
    Symbol::new("prolog")
}

#[cfg(test)]
mod tests {
    use sim_kernel::{ClaimKind, ClaimPattern, Expr, NumberLiteral, Ref, testing::bare_cx as cx};
    use sim_lib_lang_genconf::generated_coverage_profile_symbol;
    use sim_lib_standard_core::{standard_test_capability, standard_test_result_predicate};

    use super::*;
    use crate::run_prolog_matrix_row;

    #[test]
    fn generated_coverage_publishes_claim_without_changing_curated_counts() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let curated = run_prolog_matrix_row(&mut cx).unwrap();
        let coverage = run_prolog_generated_coverage(&mut cx).unwrap();

        assert_eq!(curated.pass_count(), 16);
        assert_eq!(curated.gap_count(), 3);
        assert_eq!(curated.fail_count(), 0);
        assert!(coverage.report.sampled > 0);
        assert!(coverage.report.round_tripped > 0);
        assert!(coverage.report.coverage().is_some_and(|value| value > 0.0));
        let claims = cx
            .query_facts(ClaimPattern {
                subject: Some(Ref::Symbol(generated_coverage_profile_symbol(
                    &coverage.report.language,
                ))),
                predicate: Some(standard_test_result_predicate()),
                object: None,
                include_revoked: false,
            })
            .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].kind, ClaimKind::Observed);
    }

    #[test]
    fn generated_card_fields_are_separate_from_curated_fields() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let fields = prolog_generated_coverage_card_fields(&mut cx).unwrap();

        assert!(missing_field(&fields, "conformance.fidelity"));
        assert_ne!(
            number_field(&mut cx, &fields, "coverage.generated.sampled"),
            "0"
        );
        assert_ne!(
            number_field(&mut cx, &fields, "coverage.generated.round-trip"),
            "0"
        );
        let percent = table_value(&mut cx, &fields, "coverage.generated.percent");
        assert_ne!(percent, Expr::String("unanchored".to_owned()));
        assert_eq!(
            table_value(&mut cx, &fields, "coverage.generated.citation"),
            Expr::String("expr-space/core".to_owned())
        );
    }

    fn missing_field(fields: &[(Symbol, sim_kernel::Value)], name: &str) -> bool {
        fields.iter().all(|(field, _)| field != &Symbol::new(name))
    }

    fn number_field(cx: &mut Cx, fields: &[(Symbol, sim_kernel::Value)], name: &str) -> String {
        let Expr::Number(NumberLiteral { domain, canonical }) = table_value(cx, fields, name)
        else {
            panic!("expected number field {name}");
        };
        assert_eq!(domain, Symbol::qualified("numbers", "u64"));
        canonical
    }

    fn table_value(cx: &mut Cx, fields: &[(Symbol, sim_kernel::Value)], name: &str) -> Expr {
        fields
            .iter()
            .find_map(|(field, value)| {
                (field == &Symbol::new(name)).then(|| value.object().as_expr(cx).unwrap())
            })
            .unwrap_or_else(|| panic!("missing field {name}"))
    }
}
