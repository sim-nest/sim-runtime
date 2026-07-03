use std::sync::Arc;

use sim_kernel::{
    ClaimKind, ClaimPattern, Cx, DefaultFactory, Expr, NoopEvalPolicy, Ref, Symbol,
    card::{card_for_ref, card_tests_predicate},
    standard::standard_evidence_predicate,
};

use crate::{
    ConformanceHarness, ConformanceOutcome, ConformanceTestCase, FidelityBadge, LanguageProfile,
    OrganUse, StandardTestReport, standard_binding_organ_symbol,
    standard_reported_fidelity_level_predicate, standard_test_capability,
    standard_test_result_predicate, standard_test_run_kind, standard_test_status_predicate,
    standard_test_stub,
};

#[test]
fn standard_test_reports_per_organ_pass_fail() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let profile = conformance_profile(profile_symbol());

    let report = standard_test_stub(&mut cx, &conformance_harness(false), &profile).unwrap();

    assert!(!report.passed());
    assert_eq!(report.result_count(), 2);
    assert!(organ_report(&report, control_organ()).passed());
    let binding = organ_report(&report, standard_binding_organ_symbol());
    assert!(!binding.passed());
    assert_eq!(
        binding.tests[0].detail.as_deref(),
        Some("binding regression")
    );
}

#[test]
fn failed_organ_tests_lower_reported_badge() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let profile = conformance_profile(profile_symbol());

    let report = standard_test_stub(&mut cx, &conformance_harness(false), &profile).unwrap();

    let control_badge = reported_badge(&report, Symbol::qualified("standard", "control"));
    assert_eq!(control_badge.level, 2);
    assert_eq!(control_badge.evidence, Ref::Symbol(control_test_symbol()));

    let binding_badge = reported_badge(&report, binding_badge_symbol());
    let failed_evidence = organ_report(&report, standard_binding_organ_symbol()).tests[0]
        .evidence
        .clone();
    assert_eq!(binding_badge.level, 1);
    assert_eq!(binding_badge.evidence, failed_evidence);
}

#[test]
fn harness_is_profile_agnostic_and_organ_keyed() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let mut harness = ConformanceHarness::new();
    harness.register_test(pass_case(control_test_symbol(), control_organ()));

    assert_eq!(harness.test_count(), 1);
    assert_eq!(harness.tests_for_organ(&control_organ()).len(), 1);
    assert!(
        harness
            .tests_for_organ(&standard_binding_organ_symbol())
            .is_empty()
    );

    let first =
        standard_test_stub(&mut cx, &harness, &conformance_profile(profile_symbol())).unwrap();
    let second = standard_test_stub(
        &mut cx,
        &harness,
        &conformance_profile(Symbol::qualified("lang", "other-conformance/v1")),
    )
    .unwrap();
    assert_eq!(first.result_count(), 1);
    assert_eq!(second.result_count(), 1);
    assert!(first.passed());
    assert!(second.passed());
}

#[test]
fn standard_test_publishes_cards_and_evidence_claims() {
    let mut cx = test_cx();
    cx.grant(standard_test_capability());
    let profile = conformance_profile(profile_symbol());

    let report = standard_test_stub(&mut cx, &conformance_harness(false), &profile).unwrap();
    let failed_evidence = organ_report(&report, standard_binding_organ_symbol()).tests[0]
        .evidence
        .clone();
    let evidence_card = card_for_ref(&mut cx, failed_evidence.clone())
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();

    assert_eq!(
        table_value(&evidence_card, "kind"),
        Some(&Expr::Symbol(standard_test_run_kind()))
    );
    assert_list_contains_symbol(
        table_value(&evidence_card, "tests").unwrap(),
        binding_test_symbol(),
    );
    assert_has_claim(
        &cx,
        failed_evidence.clone(),
        card_tests_predicate(),
        Ref::Symbol(binding_test_symbol()),
    );
    assert_has_claim(
        &cx,
        Ref::Symbol(profile.symbol.clone()),
        standard_test_result_predicate(),
        failed_evidence.clone(),
    );
    assert_has_claim(
        &cx,
        Ref::Symbol(profile.symbol.clone()),
        standard_evidence_predicate(),
        failed_evidence.clone(),
    );
    assert_has_claim(
        &cx,
        failed_evidence.clone(),
        standard_test_status_predicate(),
        Ref::Symbol(Symbol::qualified("standard/test", "fail")),
    );
    let level_claims = cx
        .query_facts(ClaimPattern::exact(
            Ref::Symbol(profile.symbol.clone()),
            standard_reported_fidelity_level_predicate(),
            Ref::Symbol(Symbol::qualified("standard/fidelity-level", "1")),
        ))
        .unwrap();
    assert_eq!(level_claims.len(), 1);
    assert_eq!(level_claims[0].kind, ClaimKind::Observed);
    assert_eq!(level_claims[0].evidence, vec![failed_evidence]);
}

fn conformance_profile(profile: Symbol) -> LanguageProfile {
    LanguageProfile::new(profile.clone())
        .with_reader(Symbol::qualified("codec", "lisp"))
        .with_lowering(Symbol::qualified("standard", "identity-lowering"))
        .with_eval_policy(Symbol::qualified("eval", "noop"))
        .with_organ(OrganUse::new(control_organ()))
        .with_organ(OrganUse::new(standard_binding_organ_symbol()))
        .with_conformance_test(control_test_symbol())
        .with_conformance_test(binding_test_symbol())
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile.clone()),
            Symbol::qualified("standard", "control"),
            2,
            Ref::Symbol(control_test_symbol()),
        ))
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile),
            binding_badge_symbol(),
            2,
            Ref::Symbol(binding_test_symbol()),
        ))
}

fn conformance_harness(binding_passes: bool) -> ConformanceHarness {
    let mut harness = ConformanceHarness::new();
    harness.register_test(pass_case(control_test_symbol(), control_organ()));
    let binding = if binding_passes {
        pass_case(binding_test_symbol(), standard_binding_organ_symbol())
    } else {
        fail_case(
            binding_test_symbol(),
            standard_binding_organ_symbol(),
            "binding regression",
        )
        .affecting_badge(binding_badge_symbol())
    };
    harness.register_test(binding);
    harness
}

fn pass_case(test: Symbol, organ: Symbol) -> ConformanceTestCase {
    ConformanceTestCase::new(test, organ, Arc::new(|_, _| Ok(ConformanceOutcome::pass())))
}

fn fail_case(test: Symbol, organ: Symbol, detail: &'static str) -> ConformanceTestCase {
    ConformanceTestCase::new(
        test,
        organ,
        Arc::new(move |_, _| Ok(ConformanceOutcome::fail(detail))),
    )
}

fn organ_report(report: &StandardTestReport, organ: Symbol) -> &crate::OrganTestReport {
    report
        .organs
        .iter()
        .find(|reported| reported.organ == organ)
        .unwrap()
}

fn reported_badge(report: &StandardTestReport, badge: Symbol) -> &FidelityBadge {
    report
        .reported_badges
        .iter()
        .find(|reported| reported.badge == badge)
        .unwrap()
}

fn control_organ() -> Symbol {
    Symbol::qualified("organ", "control")
}

fn profile_symbol() -> Symbol {
    Symbol::qualified("lang", "conformance/v1")
}

fn control_test_symbol() -> Symbol {
    Symbol::qualified("test", "control-pass")
}

fn binding_test_symbol() -> Symbol {
    Symbol::qualified("test", "binding-fail")
}

fn binding_badge_symbol() -> Symbol {
    Symbol::qualified("standard", "binding")
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
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

fn assert_list_contains_symbol(expr: &Expr, expected: Symbol) {
    let Expr::List(items) = expr else {
        panic!("expected list");
    };
    assert!(
        items
            .iter()
            .any(|item| item == &Expr::Symbol(expected.clone())),
        "expected list to contain {expected}"
    );
}

fn assert_has_claim(cx: &Cx, subject: Ref, predicate: Symbol, object: Ref) {
    let claims = cx
        .query_facts(ClaimPattern::exact(subject, predicate, object))
        .unwrap();
    assert_eq!(claims.len(), 1);
}
