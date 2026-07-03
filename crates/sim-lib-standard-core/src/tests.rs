use std::sync::Arc;

use sim_codec::{Input, decode_with_codec};
use sim_codec_lisp::{LispCodecLib, encode_object_lisp};
use sim_kernel::{
    CapabilitySet, ClaimPattern, ContentId, Cx, DefaultFactory, EncodeOptions, EncodePosition,
    Error, Expr, HandleId, NoopEvalPolicy, ReadPolicy, Ref, Symbol, TrustLevel, WriteCx,
    card::card_for_ref, card::card_kind_predicate, read_construct_capability,
    standard::standard_organ_kind, standard::standard_profile_kind,
};

use crate::{
    FidelityBadge, FidelityBadgeValue, LanguageProfile, LanguageProfileValue, OrganUse,
    ProfileDiffStatus, ProfileRegistry, SharedOrganRuntime, fidelity_badge_class_symbol,
    install_profile_stub, install_standard_core_classes, language_profile_class_symbol,
    language_profile_lib_symbol, profile_function_value, sim_expression_profile,
    sim_expression_profile_symbol, standard_binding_organ_symbol, standard_core_classes_lib_symbol,
    standard_diff_capability, standard_diff_stub, standard_install_capability,
};

#[test]
fn profile_registers_by_symbol() {
    let mut registry = ProfileRegistry::new();
    let profile = sample_profile();

    registry.register_profile(profile.clone()).unwrap();

    assert_eq!(
        registry.profile(&Symbol::qualified("lang", "sample/v1")),
        Some(&profile)
    );
    assert!(registry.register_profile(profile).is_err());
}

#[test]
fn profile_card_is_claim_derived() {
    let mut cx = test_cx();
    cx.grant(standard_install_capability());
    let mut registry = ProfileRegistry::new();
    let profile = sample_profile();

    install_profile_stub(&mut cx, &mut registry, profile.clone()).unwrap();

    let card = card_for_ref(&mut cx, Ref::Symbol(profile.symbol.clone()))
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();
    assert_eq!(
        table_value(&card, "kind"),
        Some(&Expr::Symbol(standard_profile_kind()))
    );
    assert_list_contains_symbol(
        table_value(&card, "tests").unwrap(),
        Symbol::qualified("test", "sample-conformance"),
    );
    assert_list_contains_symbol(
        table_value(&card, "requires").unwrap(),
        Symbol::qualified("capability", "standard.test"),
    );
}

#[test]
fn profile_install_claims_unload_with_profile_lib() {
    let mut cx = test_cx();
    cx.grant(standard_install_capability());
    let profile = sample_profile();
    let lib_symbol = language_profile_lib_symbol(&profile.symbol);

    let mut registry = ProfileRegistry::new();
    install_profile_stub(&mut cx, &mut registry, profile.clone()).unwrap();
    let lib_id = cx.registry().lib(&lib_symbol).unwrap().id;
    assert!(
        !cx.query_facts(profile_kind_claim(&profile.symbol))
            .unwrap()
            .is_empty()
    );

    cx.unload_lib(lib_id).unwrap();
    assert!(cx.registry().lib(&lib_symbol).is_none());
    assert!(
        cx.query_facts(profile_kind_claim(&profile.symbol))
            .unwrap()
            .is_empty()
    );

    let mut registry = ProfileRegistry::new();
    install_profile_stub(&mut cx, &mut registry, profile.clone()).unwrap();
    assert!(cx.registry().lib(&lib_symbol).is_some());
    assert!(
        !cx.query_facts(profile_kind_claim(&profile.symbol))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn profile_and_badge_read_constructors_round_trip_values() {
    let mut cx = test_cx();
    install_standard_core_classes(&mut cx).unwrap();
    cx.grant(read_construct_capability());
    let profile = sample_profile();
    let badge = profile.fidelity_badges[0].clone();

    let profile_args = expr_values(&mut cx, profile.to_constructor_args());
    let profile_value = cx
        .read_construct(&language_profile_class_symbol(), profile_args)
        .unwrap();
    let profile_roundtrip = profile_value
        .object()
        .downcast_ref::<LanguageProfileValue>()
        .unwrap();
    assert_eq!(profile_roundtrip.profile(), &profile);

    let badge_args = expr_values(&mut cx, badge.to_constructor_args());
    let badge_value = cx
        .read_construct(&fidelity_badge_class_symbol(), badge_args)
        .unwrap();
    let badge_roundtrip = badge_value
        .object()
        .downcast_ref::<FidelityBadgeValue>()
        .unwrap();
    assert_eq!(badge_roundtrip.badge(), &badge);

    let content_badge = FidelityBadge::new(
        Ref::Content(ContentId::from_bytes(
            Symbol::qualified("test", "hash"),
            [7; 32],
        )),
        Symbol::qualified("standard", "content-evidence"),
        3,
        Ref::Handle(HandleId(42)),
    );
    let content_badge_args = expr_values(&mut cx, content_badge.to_constructor_args());
    let content_badge_value = cx
        .read_construct(&fidelity_badge_class_symbol(), content_badge_args)
        .unwrap();
    let content_badge_roundtrip = content_badge_value
        .object()
        .downcast_ref::<FidelityBadgeValue>()
        .unwrap();
    assert_eq!(content_badge_roundtrip.badge(), &content_badge);
}

#[test]
fn standard_core_classes_unload_and_reload() {
    let mut cx = test_cx();
    cx.grant(read_construct_capability());
    let profile = sample_profile();
    let profile_args = expr_values(&mut cx, profile.to_constructor_args());

    install_standard_core_classes(&mut cx).unwrap();
    let lib_id = cx
        .registry()
        .lib(&standard_core_classes_lib_symbol())
        .unwrap()
        .id;
    assert!(
        cx.read_construct(&language_profile_class_symbol(), profile_args.clone())
            .is_ok()
    );

    cx.unload_lib(lib_id).unwrap();
    assert!(
        cx.read_construct(&language_profile_class_symbol(), profile_args.clone())
            .is_err()
    );

    install_standard_core_classes(&mut cx).unwrap();
    assert!(
        cx.read_construct(&language_profile_class_symbol(), profile_args)
            .is_ok()
    );
}

#[test]
fn profile_read_construct_round_trips_through_lisp_codec() {
    let mut cx = test_cx();
    install_lisp_codec(&mut cx);
    install_standard_core_classes(&mut cx).unwrap();
    cx.grant(read_construct_capability());
    let profile = sample_profile();
    let value = cx
        .factory()
        .opaque(Arc::new(LanguageProfileValue::new(profile.clone())))
        .unwrap();

    let mut write = WriteCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(1),
        options: EncodeOptions {
            position: EncodePosition::Quote,
            ..Default::default()
        },
    };
    let encoded = encode_object_lisp(&mut write, value).unwrap();
    assert!(encoded.starts_with("#(standard/Profile "));

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(encoded),
        read_policy_with_construct(),
    )
    .unwrap();
    let Expr::Call { operator, args } = decoded else {
        panic!("expected profile constructor expression");
    };
    assert_eq!(*operator, Expr::Symbol(language_profile_class_symbol()));
    assert_eq!(
        LanguageProfile::from_constructor_args(args).unwrap(),
        profile
    );
}

#[test]
fn standard_core_does_not_publish_organ_implementation_claims() {
    let mut cx = test_cx();
    cx.grant(standard_install_capability());
    let mut registry = ProfileRegistry::new();
    let profile = sample_profile();
    let organ = profile.organs[0].organ.clone();

    install_profile_stub(&mut cx, &mut registry, profile).unwrap();

    let organ_kind_claims = cx
        .query_facts(ClaimPattern::exact(
            Ref::Symbol(organ),
            card_kind_predicate(),
            Ref::Symbol(standard_organ_kind()),
        ))
        .unwrap();
    assert!(organ_kind_claims.is_empty());
}

#[test]
fn sim_expression_profile_is_second_small_profile() {
    let profile = sim_expression_profile();

    assert_eq!(profile.symbol, sim_expression_profile_symbol());
    assert_eq!(profile.reader, Symbol::qualified("codec", "lisp"));
    assert!(profile.unsupported_forms.is_empty());
    assert!(
        profile
            .organs
            .iter()
            .any(|organ| { organ.organ == Symbol::qualified("organ", "control") })
    );
    assert!(
        profile
            .organs
            .iter()
            .any(|organ| { organ.organ == standard_binding_organ_symbol() })
    );
}

#[test]
fn profile_diff_reports_same_or_structured_difference() {
    let mut cx = test_cx();
    cx.grant(standard_diff_capability());
    let profile = sample_profile();

    let same = standard_diff_stub(&cx, &profile, &profile).unwrap();
    assert_eq!(same.status, ProfileDiffStatus::Same);
    assert!(same.is_same());
    assert!(same.differences.is_empty());

    let diff = standard_diff_stub(&cx, &profile, &sim_expression_profile()).unwrap();
    assert_eq!(diff.status, ProfileDiffStatus::Different);
    assert!(!diff.is_same());
    assert!(
        diff.shared_organs
            .contains(&Symbol::qualified("organ", "control"))
    );
    assert!(
        diff.differences
            .iter()
            .any(|difference| difference.field == Symbol::qualified("profile/diff", "organs"))
    );
    assert!(
        diff.differences
            .iter()
            .any(|difference| difference.field == Symbol::qualified("profile/diff", "numeric"))
    );
}

#[test]
fn shared_organ_runtime_calls_across_profiles_without_value_conversion() {
    let mut cx = test_cx();
    let defining_profile = sample_profile();
    let calling_profile = sim_expression_profile();
    let mut runtime = SharedOrganRuntime::new();
    let function = Symbol::qualified("test", "polyglot-identity");
    let organ = Symbol::qualified("organ", "control");
    runtime.register_profile(defining_profile.clone()).unwrap();
    runtime.register_profile(calling_profile.clone()).unwrap();
    let callable = profile_function_value(
        &mut cx,
        defining_profile.symbol.clone(),
        organ.clone(),
        function.clone(),
        |_cx, args| {
            args.values()
                .first()
                .cloned()
                .ok_or_else(|| Error::Eval("polyglot identity expects one argument".to_owned()))
        },
    )
    .unwrap();

    runtime
        .define_function(&defining_profile.symbol, organ, function.clone(), callable)
        .unwrap();
    let input = cx
        .factory()
        .string("same runtime value".to_owned())
        .unwrap();
    let result = runtime
        .call_function(
            &mut cx,
            &calling_profile.symbol,
            &function,
            vec![input.clone()],
        )
        .unwrap();

    assert_eq!(result, input);
}

fn sample_profile() -> LanguageProfile {
    let control = Symbol::qualified("organ", "control");
    LanguageProfile::new(Symbol::qualified("lang", "sample/v1"))
        .with_reader(Symbol::qualified("codec", "lisp"))
        .with_lowering(Symbol::qualified("standard", "identity-lowering"))
        .with_eval_policy(Symbol::qualified("eval", "noop"))
        .with_organ(
            OrganUse::new(control.clone()).with_option(Symbol::new("multishot"), Expr::Bool(false)),
        )
        .with_numeric_tower(Symbol::qualified("numbers", "none"))
        .requiring(crate::standard_test_capability())
        .with_unsupported_form(Symbol::qualified("sample", "call/cc"))
        .with_conformance_test(Symbol::qualified("test", "sample-conformance"))
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(control),
            Symbol::qualified("standard", "one-shot"),
            2,
            Ref::Symbol(Symbol::qualified("test", "sample-conformance")),
        ))
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn expr_values(cx: &mut Cx, exprs: Vec<Expr>) -> Vec<sim_kernel::Value> {
    exprs
        .into_iter()
        .map(|expr| cx.factory().expr(expr).unwrap())
        .collect()
}

fn install_lisp_codec(cx: &mut Cx) {
    let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lib).unwrap();
}

fn read_policy_with_construct() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: CapabilitySet::new().grant(read_construct_capability()),
    }
}

fn profile_kind_claim(profile: &Symbol) -> ClaimPattern {
    ClaimPattern::exact(
        Ref::Symbol(profile.clone()),
        card_kind_predicate(),
        Ref::Symbol(standard_profile_kind()),
    )
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
