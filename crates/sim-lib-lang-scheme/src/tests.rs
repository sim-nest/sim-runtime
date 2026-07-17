use sim_codec::{Input, decode_tree_with_codec};
use sim_kernel::{
    CapabilitySet, ClaimPattern, Datum, Error, Expr, ReadPolicy, Ref, Symbol, Term, TrustLevel,
    card::{card_for_ref, card_kind_predicate},
    standard::standard_profile_kind,
};
use sim_lib_standard_core::{
    ProfileRegistry, SharedOrganRuntime, profile_function_value, sim_expression_profile,
    standard_binding_organ_symbol, standard_test_capability, standard_test_result_predicate,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

#[test]
fn required_forms_have_documented_shapes() {
    let mut cx = cx();
    let forms = r7rs_small_form_specs();
    assert!(forms.len() >= 10);
    for form in forms {
        assert!(!form.doc.is_empty(), "{} doc", form.symbol);
        let doc = form.shape.describe(&mut cx).unwrap();
        assert!(!doc.name.is_empty(), "{} shape", form.symbol);
    }
}

#[test]
fn reader_preserves_source_locations() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&SchemeCodecLib::new(codec_id)).unwrap();

    let tree = decode_tree_with_codec(
        &mut cx,
        &scheme_reader_symbol(),
        Input::Text("(define x 1)".to_owned()),
        read_policy(),
        "unit.scm",
    )
    .unwrap();

    let origin = tree.origin.expect("top origin");
    assert_eq!(origin.source.0.as_str(), "unit.scm");
    assert_eq!(origin.span.start, 0);
    assert_eq!(origin.span.end, "(define x 1)".len());
    assert_eq!(tree.children.len(), 3);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.start, 8);
}

#[test]
fn string_literal_preserves_non_ascii() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&SchemeCodecLib::new(codec_id)).unwrap();

    // Source holds a 2-byte UTF-8 scalar inside the quotes; kept ASCII-only in
    // this file via a `\u{..}` escape (R8). It must decode, not turn to mojibake.
    let tree = decode_tree_with_codec(
        &mut cx,
        &scheme_reader_symbol(),
        Input::Text("\"caf\u{00e9}\"".to_owned()),
        read_policy(),
        "unicode.scm",
    )
    .unwrap();
    assert_eq!(tree.expr, Expr::String("caf\u{00e9}".to_owned()));
}

#[test]
fn core_forms_lower_to_canonical_term_or_datum() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&SchemeCodecLib::new(codec_id)).unwrap();
    let quoted = decode_tree_with_codec(
        &mut cx,
        &scheme_reader_symbol(),
        Input::Text("'answer".to_owned()),
        read_policy(),
        "quote.scm",
    )
    .unwrap();
    let datum = lower_scheme_tree(&quoted).unwrap();
    assert_eq!(
        datum.lowered,
        SchemeLowered::Datum(Datum::Symbol(Symbol::new("answer")))
    );

    let begin = decode_tree_with_codec(
        &mut cx,
        &scheme_reader_symbol(),
        Input::Text("(begin #t #f)".to_owned()),
        read_policy(),
        "begin.scm",
    )
    .unwrap();
    let lowered = lower_scheme_tree(&begin).unwrap();
    let SchemeLowered::Term(Term::Seq(items)) = lowered.lowered else {
        panic!("expected begin to lower to sequence term");
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn base_exports_are_cards() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_r7rs_small_profile(&mut cx, &mut registry).unwrap();

    let profile_kind = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol.clone())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_profile_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(profile_kind.len(), 1);

    let export = Symbol::qualified("scheme", "if");
    let card = card_for_ref(&mut cx, Ref::Symbol(export)).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();
    assert_eq!(
        table_value(&expr, "kind"),
        Some(&Expr::Symbol(scheme_base_export_kind_symbol()))
    );
}

#[test]
fn unsupported_forms_produce_profile_diagnostics() {
    let mut cx = cx();
    let mut read_cx = sim_codec::ReadCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(0),
        read_policy: read_policy(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let tree = decode_scheme_tree(
        &mut read_cx,
        "unsupported.scm",
        Input::Text("(call/cc k)".to_owned()),
    )
    .unwrap();
    let diagnostics = diagnose_unsupported_forms(&tree.expr);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code,
        Some(Symbol::qualified("scheme", "unsupported-form"))
    );
}

#[test]
fn profile_runs_under_restricted_capabilities() {
    let mut cx = cx();
    let lowered = run_r7rs_small_restricted(&mut cx, "(begin #t #f)").unwrap();
    let SchemeLowered::Term(Term::Seq(items)) = lowered else {
        panic!("expected sequence term");
    };
    assert_eq!(items.len(), 2);
    assert!(run_r7rs_small_restricted(&mut cx, "(eval '(+ 1 2))").is_err());
}

#[test]
fn r7rs_profile_shares_organs_with_sim_expression_profile() {
    let mut cx = cx();
    let scheme = r7rs_small_profile();
    let sim_expression = sim_expression_profile();
    let mut runtime = SharedOrganRuntime::new();
    let organ = standard_binding_organ_symbol();
    let function = Symbol::qualified("test", "scheme-defined-identity");
    runtime.register_profile(scheme.clone()).unwrap();
    runtime.register_profile(sim_expression.clone()).unwrap();
    let callable = profile_function_value(
        &mut cx,
        scheme.symbol.clone(),
        organ.clone(),
        function.clone(),
        |_cx, args| {
            args.values()
                .first()
                .cloned()
                .ok_or_else(|| Error::Eval("scheme identity expects one argument".to_owned()))
        },
    )
    .unwrap();

    runtime
        .define_function(&scheme.symbol, organ, function.clone(), callable)
        .unwrap();
    let input = cx
        .factory()
        .symbol(Symbol::qualified("value", "shared"))
        .unwrap();
    let result = runtime
        .call_function(
            &mut cx,
            &sim_expression.symbol,
            &function,
            vec![input.clone()],
        )
        .unwrap();

    assert_eq!(result, input);
}

#[test]
fn scheme_matrix_row_publishes_cell_claims() {
    let mut cx = cx();
    cx.grant(standard_test_capability());

    let report = run_scheme_matrix_row(&mut cx).unwrap();

    assert_eq!(report.cells.len(), 4);
    assert_eq!(report.pass_count(), 2);
    assert_eq!(report.gap_count(), 2);
    assert_eq!(report.fail_count(), 0);
    assert_eq!(report.language_fidelity(&Symbol::new("scheme")), Some(1.0));
    assert!(
        report.cells.iter().any(|cell| {
            cell.case_symbol == Symbol::qualified("test/r7rs-small", "eval-gap")
                && cell.outcome.is_gap()
        }),
        "expected observed source gap cell",
    );
    assert!(
        report.cells.iter().any(|cell| {
            cell.case_symbol == Symbol::qualified("test/r7rs-small", "expr-callcc-gap")
                && cell.outcome.is_gap()
        }),
        "expected observed expression gap cell",
    );
    let claims = cx.query_facts(scheme_profile_result_claims()).unwrap();
    assert_eq!(claims.len(), 4);
}

#[test]
fn scheme_card_without_capability_emits_unscored() {
    let mut cx = cx();

    let card = scheme_language_card(&mut cx).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(number_table_value(&expr, "conformance.pass"), Some("0"));
    assert_eq!(number_table_value(&expr, "conformance.gap"), Some("0"));
    assert_eq!(number_table_value(&expr, "conformance.fail"), Some("0"));
    assert_eq!(
        table_value(&expr, "conformance.fidelity"),
        Some(&Expr::String("unscored".to_owned()))
    );
}

#[test]
fn scheme_card_with_capability_emits_fidelity() {
    let mut cx = cx();
    cx.grant(standard_test_capability());

    let card = scheme_language_card(&mut cx).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(number_table_value(&expr, "conformance.pass"), Some("2"));
    assert_eq!(number_table_value(&expr, "conformance.gap"), Some("2"));
    assert_eq!(number_table_value(&expr, "conformance.fail"), Some("0"));
    assert_eq!(
        table_value(&expr, "conformance.fidelity"),
        Some(&Expr::String("100%".to_owned()))
    );
    let claims = cx.query_facts(scheme_profile_result_claims()).unwrap();
    assert_eq!(claims.len(), 4);
}

#[test]
fn scheme_card_generated_coverage_appears_next_to_fidelity() {
    let mut cx = cx();
    cx.grant(standard_test_capability());
    let generated_fields = vec![
        (
            Symbol::new("coverage.generated.sampled"),
            cx.factory()
                .number_literal(Symbol::qualified("numbers", "u64"), "4".to_owned())
                .unwrap(),
        ),
        (
            Symbol::new("coverage.generated.round-trip"),
            cx.factory()
                .number_literal(Symbol::qualified("numbers", "u64"), "3".to_owned())
                .unwrap(),
        ),
        (
            Symbol::new("coverage.generated.percent"),
            cx.factory().string("75%".to_owned()).unwrap(),
        ),
        (
            Symbol::new("coverage.generated.citation"),
            cx.factory().string("r7rs-small".to_owned()).unwrap(),
        ),
    ];

    let card = scheme_language_card_with_generated_coverage(&mut cx, generated_fields).unwrap();
    let expr = card.object().as_expr(&mut cx).unwrap();

    assert_eq!(
        table_value(&expr, "conformance.fidelity"),
        Some(&Expr::String("100%".to_owned()))
    );
    assert_eq!(
        table_value(&expr, "coverage.generated.percent"),
        Some(&Expr::String("75%".to_owned()))
    );
    assert_eq!(
        number_table_value(&expr, "coverage.generated.sampled"),
        Some("4")
    );
}

#[test]
fn scheme_matrix_row_claims_are_evidence_backed_and_browseable() {
    let mut cx = cx();
    cx.grant(standard_test_capability());

    let report = run_scheme_matrix_row(&mut cx).expect("scheme matrix row must run");

    assert!(
        report.pass_count() + report.gap_count() > 0,
        "must have at least one pass or gap"
    );
    let language = Symbol::new("scheme");
    let fields = report.conformance_card_fields(&mut cx, &language).unwrap();
    let fidelity = string_field_value(&mut cx, &fields, "conformance.fidelity");
    assert!(
        fidelity.ends_with('%') || fidelity == "unscored",
        "fidelity must be a percentage or unscored, got: {fidelity}",
    );
    let claims = cx.query_facts(scheme_profile_result_claims()).unwrap();
    assert!(
        !claims.is_empty(),
        "matrix run must publish at least one evidence claim",
    );
}

fn read_policy() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: CapabilitySet::new(),
    }
}

fn table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a Expr> {
    let Expr::Map(entries) = expr else {
        return None;
    };
    entries.iter().find_map(|(entry_key, entry_value)| {
        let Expr::Symbol(entry_key) = entry_key else {
            return None;
        };
        (entry_key == &Symbol::new(key)).then_some(entry_value)
    })
}

fn number_table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a str> {
    let value = table_value(expr, key)?;
    let Expr::Number(number) = value else {
        return None;
    };
    assert_eq!(number.domain, Symbol::qualified("numbers", "u64"));
    Some(number.canonical.as_str())
}

fn string_field_value(
    cx: &mut sim_kernel::Cx,
    fields: &[(Symbol, sim_kernel::Value)],
    name: &str,
) -> String {
    let Expr::String(value) = fields
        .iter()
        .find(|(field, _)| field == &Symbol::new(name))
        .unwrap_or_else(|| panic!("missing field {name}"))
        .1
        .object()
        .as_expr(cx)
        .unwrap()
    else {
        panic!("expected string field {name}");
    };
    value
}

fn scheme_profile_result_claims() -> ClaimPattern {
    ClaimPattern {
        subject: Some(Ref::Symbol(r7rs_small_profile_symbol())),
        predicate: Some(standard_test_result_predicate()),
        object: None,
        include_revoked: false,
    }
}
