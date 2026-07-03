use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{
    Cx, Expr, Ref, Symbol, Value,
    card::{card_for_ref, card_kind_predicate},
    standard::standard_profile_kind,
};
use sim_lib_standard_core::ProfileRegistry;
use sim_shape::{AnyShape, ExprKindShape};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn body(label: &'static str) -> sim_lib_dispatch::MethodBody {
    Arc::new(move |cx, _args| cx.factory().string(label.to_owned()))
}

fn string_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String))
}

fn any_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(AnyShape)
}

#[test]
fn islisp_generics_delegate_to_dispatch_organ() {
    let mut cx = cx();
    let mut generic = IslispGeneric::new(Symbol::qualified("islisp", "describe-object"));
    generic
        .add_primary_method(
            Symbol::qualified("method", "broad"),
            vec![any_shape()],
            body("broad"),
        )
        .unwrap();
    generic
        .add_primary_method(
            Symbol::qualified("method", "string"),
            vec![string_shape()],
            body("string"),
        )
        .unwrap();

    let args = [string(&mut cx, "sample")];
    let selected = generic.select_primary(&mut cx, &args).unwrap();
    assert_eq!(selected.method(), &Symbol::qualified("method", "string"));
    assert_eq!(
        generic.dispatch_order(&mut cx, &args).unwrap(),
        vec![Symbol::qualified("method", "string")]
    );
    assert_eq!(
        generic
            .call(&mut cx, &args)
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("string".to_owned())
    );

    let profile = islisp_profile();
    assert!(
        profile
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_dispatch::dispatch_organ_symbol())
    );
}

#[test]
fn object_and_generic_forms_are_documented() {
    let specs = islisp_form_specs();
    assert_eq!(specs.len(), 3);
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("islisp", "defclass")
            && spec.role == IslispFormRole::Object
            && !spec.doc.is_empty()
    }));
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("islisp", "defgeneric")
            && spec.role == IslispFormRole::Generic
            && spec.organ == sim_lib_dispatch::dispatch_organ_symbol()
    }));
}

#[test]
fn object_values_round_trip_as_profile_data() {
    let mut cx = cx();
    let mut slots = BTreeMap::new();
    slots.insert(Symbol::new("name"), string(&mut cx, "unit"));
    let object = islisp_object_value(&mut cx, Symbol::qualified("class", "sample"), slots).unwrap();
    let expr = object.object().as_expr(&mut cx).unwrap();
    let Expr::Map(entries) = expr else {
        panic!("expected object map expression");
    };
    assert!(entries.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("class"))
            && value == &Expr::Symbol(Symbol::qualified("class", "sample"))
    }));
}

#[test]
fn islisp_profile_publishes_per_organ_fidelity() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_islisp_profile(&mut cx, &mut registry).unwrap();

    let profile_kind = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol.clone())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_profile_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(profile_kind.len(), 1);

    let fidelity = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol)),
            predicate: Some(Symbol::qualified("standard", "fidelity-badge")),
            object: Some(Ref::Symbol(islisp_dispatch_fidelity_symbol())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(fidelity.len(), 1);

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
