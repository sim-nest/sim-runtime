use std::sync::Arc;

use sim_kernel::{
    Cx, Expr, Ref, Symbol, Value,
    card::{card_for_ref, card_kind_predicate},
    standard::standard_profile_kind,
};
use sim_lib_dispatch::MethodBody;
use sim_lib_standard_core::ProfileRegistry;
use sim_shape::{AnyShape, ExprKindShape};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn body(label: &'static str) -> MethodBody {
    Arc::new(move |cx, _args| cx.factory().string(label.to_owned()))
}

#[test]
fn julia_specificity_reuses_dispatch_organ() {
    let mut cx = cx();
    let mut function = JuliaFunction::new(Symbol::qualified("julia", "show"));
    function
        .add_method(
            Symbol::qualified("method", "any"),
            vec![Arc::new(AnyShape)],
            body("any"),
        )
        .unwrap();
    function
        .add_method(
            Symbol::qualified("method", "string"),
            vec![Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String))],
            body("string"),
        )
        .unwrap();

    let args = [string(&mut cx, "text")];
    assert_eq!(
        function.dispatch_order(&mut cx, &args).unwrap(),
        vec![Symbol::qualified("method", "string")]
    );
    assert_eq!(
        function
            .call(&mut cx, &args)
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("string".to_owned())
    );
    assert!(
        julia_core_profile()
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_dispatch::dispatch_organ_symbol())
    );
}

#[test]
fn julia_profile_uses_algol_reader_and_honest_badges() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_julia_core_profile(&mut cx, &mut registry).unwrap();

    assert_eq!(profile.reader, Symbol::qualified("codec", "algol"));
    assert!(
        profile.fidelity_badges.iter().any(|badge| {
            badge.badge == julia_full_runtime_fidelity_symbol() && badge.level == 0
        })
    );
    assert_eq!(
        cx.query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol)),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_profile_kind())),
            include_revoked: false,
        })
        .unwrap()
        .len(),
        1
    );
    let card = card_for_ref(
        &mut cx,
        Ref::Symbol(sim_lib_dispatch::dispatch_organ_symbol()),
    )
    .unwrap()
    .object()
    .as_expr(&mut cx)
    .unwrap();
    assert!(matches!(card, Expr::Map(_)));
}
