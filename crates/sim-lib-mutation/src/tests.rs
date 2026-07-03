use sim_kernel::{
    Cx, Error, Expr, Ref, Symbol, Table, Value,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};
use sim_lib_sequence::persistent_vector;

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

#[test]
fn mutation_requires_capability_and_then_updates() {
    let mut cx = cx();
    let cell = Cell::new(string(&mut cx, "old"));

    let new_value = string(&mut cx, "new");
    let denied = cell.set(&mut cx, new_value).unwrap_err();
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == standard_mutate_capability())
    );

    cx.grant(standard_mutate_capability());
    let new_value = string(&mut cx, "new");
    cell.set(&mut cx, new_value).unwrap();
    assert_eq!(
        cell.get().unwrap().object().as_expr(&mut cx).unwrap(),
        Expr::String("new".to_owned())
    );
}

#[test]
fn mutable_vectors_do_not_mutate_persistent_sequence_values() {
    let mut cx = cx();
    let original_value = string(&mut cx, "old");
    let original = persistent_vector(&mut cx, vec![original_value]).unwrap();
    let original_expr = original.object().as_expr(&mut cx).unwrap();
    let mutable = mutable_vector_from_value(&mut cx, &original).unwrap();
    let vector = mutable_vector_value(&mutable).unwrap();

    cx.grant(standard_mutate_capability());
    let new_value = string(&mut cx, "new");
    vector.set(&mut cx, 0, new_value).unwrap();

    assert_eq!(original.object().as_expr(&mut cx).unwrap(), original_expr);
    assert_eq!(
        mutable.object().as_expr(&mut cx).unwrap(),
        Expr::Vector(vec![Expr::String("new".to_owned())])
    );
}

#[test]
fn boxes_and_tables_are_capability_gated() {
    let mut cx = cx();
    let boxed = MutableBox::new(string(&mut cx, "old"));
    let table_value = string(&mut cx, "old");
    let table = mutable_table(&mut cx, vec![(Symbol::new("name"), table_value)]).unwrap();
    let table = mutable_table_value(&table).unwrap();

    let denied_box_value = string(&mut cx, "new");
    assert!(matches!(
        boxed.set(&mut cx, denied_box_value).unwrap_err(),
        Error::CapabilityDenied { .. }
    ));
    let denied_table_value = string(&mut cx, "new");
    assert!(matches!(
        table
            .set(&mut cx, Symbol::new("name"), denied_table_value)
            .unwrap_err(),
        Error::CapabilityDenied { .. }
    ));

    cx.grant(standard_mutate_capability());
    let boxed_value = string(&mut cx, "new");
    boxed.set(&mut cx, boxed_value).unwrap();
    let table_value = string(&mut cx, "new");
    table
        .set(&mut cx, Symbol::new("name"), table_value)
        .unwrap();
    assert_eq!(
        boxed.get().unwrap().object().as_expr(&mut cx).unwrap(),
        Expr::String("new".to_owned())
    );
    assert_eq!(
        table
            .get(&mut cx, Symbol::new("name"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("new".to_owned())
    );
}

#[test]
fn mutation_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_mutation_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(mutation_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(mutation_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "mutation organ ops").unwrap();

    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("mutation", "set.v1"))
    }));
}
