use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use sim_kernel::{
    Cx, Expr, NumberLiteral, Ref, Symbol,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn number(cx: &mut Cx, value: u64) -> sim_kernel::Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "u64"), value.to_string())
        .unwrap()
}

fn number_from_value(cx: &mut Cx, value: &sim_kernel::Value) -> u64 {
    let Expr::Number(NumberLiteral { canonical, .. }) = value.object().as_expr(cx).unwrap() else {
        panic!("expected number value");
    };
    canonical.parse().unwrap()
}

fn list_numbers(cx: &mut Cx, list: &sim_kernel::Value) -> Vec<u64> {
    let list = list.object().as_list().unwrap();
    force_list_to_vec(cx, list, "sequence test")
        .unwrap()
        .iter()
        .map(|value| number_from_value(cx, value))
        .collect()
}

fn table_number(cx: &mut Cx, table: &sim_kernel::Value, key: &str) -> u64 {
    let table = table.object().as_table_impl().unwrap();
    let value = table.get(cx, Symbol::new(key)).unwrap();
    number_from_value(cx, &value)
}

#[test]
fn persistent_ops_do_not_mutate_inputs() {
    let mut cx = cx();
    let one = number(&mut cx, 1);
    let two = number(&mut cx, 2);
    let three = number(&mut cx, 3);
    let list = persistent_list(&mut cx, vec![one, two]).unwrap();
    let extended = persistent_list_push(&mut cx, &list, three).unwrap();
    assert_eq!(list_numbers(&mut cx, &list), vec![1, 2]);
    assert_eq!(list_numbers(&mut cx, &extended), vec![1, 2, 3]);

    let four = number(&mut cx, 4);
    let five = number(&mut cx, 5);
    let four_expr = four.object().as_expr(&mut cx).unwrap();
    let five_expr = five.object().as_expr(&mut cx).unwrap();
    let vector = persistent_vector(&mut cx, vec![four]).unwrap();
    let vector2 = persistent_vector_push(&mut cx, &vector, five).unwrap();
    assert_eq!(
        vector.object().as_expr(&mut cx).unwrap(),
        Expr::Vector(vec![four_expr.clone()])
    );
    assert_eq!(
        vector2.object().as_expr(&mut cx).unwrap(),
        Expr::Vector(vec![four_expr, five_expr])
    );

    let ten = number(&mut cx, 10);
    let twenty = number(&mut cx, 20);
    let map = persistent_map(&mut cx, vec![(Symbol::new("a"), ten)]).unwrap();
    let map2 = persistent_map_assoc(&mut cx, &map, Symbol::new("a"), twenty).unwrap();
    assert_eq!(table_number(&mut cx, &map, "a"), 10);
    assert_eq!(table_number(&mut cx, &map2, "a"), 20);

    let one_a = number(&mut cx, 1);
    let one_b = number(&mut cx, 1);
    let set_two = number(&mut cx, 2);
    let one_expr = one_a.object().as_expr(&mut cx).unwrap();
    let two_expr = set_two.object().as_expr(&mut cx).unwrap();
    let set = persistent_set(&mut cx, vec![one_a, one_b]).unwrap();
    let set2 = persistent_set_insert(&mut cx, &set, set_two).unwrap();
    assert_eq!(
        set.object().as_expr(&mut cx).unwrap(),
        Expr::Set(vec![one_expr.clone()])
    );
    assert_eq!(
        set2.object().as_expr(&mut cx).unwrap(),
        Expr::Set(vec![one_expr, two_expr])
    );
}

#[test]
fn lazy_sequence_forcing_is_bounded() {
    let mut cx = cx();
    let produced = Arc::new(AtomicUsize::new(0));
    let sequence = lazy_sequence_value(&mut cx, {
        let produced = produced.clone();
        Arc::new(move |cx, index| {
            produced.fetch_add(1, Ordering::SeqCst);
            Ok(Some(number(cx, index as u64)))
        })
    })
    .unwrap();

    let err = force_sequence_bounded(&mut cx, &sequence, 3, "endless test").unwrap_err();
    assert!(format!("{err}").contains("exceeds force bound 3"));
    assert_eq!(produced.load(Ordering::SeqCst), 4);
}

#[test]
fn transducers_compose_in_one_pass() {
    let mut cx = cx();
    let produced = Arc::new(AtomicUsize::new(0));
    let mapped = Arc::new(AtomicUsize::new(0));
    let filtered = Arc::new(AtomicUsize::new(0));
    let source = lazy_sequence_value(&mut cx, {
        let produced = produced.clone();
        Arc::new(move |cx, index| {
            produced.fetch_add(1, Ordering::SeqCst);
            if index >= 5 {
                return Ok(None);
            }
            Ok(Some(number(cx, index as u64)))
        })
    })
    .unwrap();

    let pipeline = TransducerPipeline::new()
        .map({
            let mapped = mapped.clone();
            Arc::new(move |cx, value| {
                mapped.fetch_add(1, Ordering::SeqCst);
                let next = number_from_value(cx, &value) + 1;
                Ok(number(cx, next))
            })
        })
        .filter({
            let filtered = filtered.clone();
            Arc::new(move |cx, value| {
                filtered.fetch_add(1, Ordering::SeqCst);
                Ok(number_from_value(cx, value).is_multiple_of(2))
            })
        });
    let zero = number(&mut cx, 0);
    let sum = transduce(
        &mut cx,
        &source,
        pipeline,
        zero,
        Arc::new(|cx, acc, value| {
            let left = number_from_value(cx, &acc);
            let right = number_from_value(cx, &value);
            Ok(number(cx, left + right))
        }),
    )
    .unwrap();

    assert_eq!(number_from_value(&mut cx, &sum), 6);
    assert_eq!(produced.load(Ordering::SeqCst), 6);
    assert_eq!(mapped.load(Ordering::SeqCst), 5);
    assert_eq!(filtered.load(Ordering::SeqCst), 5);
}

#[test]
fn sequence_values_cross_profile_boundaries() {
    let mut cx = cx();
    let seven = number(&mut cx, 7);
    let eight = number(&mut cx, 8);
    let list = persistent_list(&mut cx, vec![seven, eight]).unwrap();
    let sequence = sequence_from_list_value(&mut cx, list).unwrap();
    let source_profile =
        sequence_for_profile(&mut cx, Symbol::qualified("profile", "source"), sequence).unwrap();
    let target_profile = sequence_for_profile(
        &mut cx,
        Symbol::qualified("profile", "target"),
        source_profile,
    )
    .unwrap();

    let values = force_sequence_bounded(&mut cx, &target_profile, 2, "profile sequence").unwrap();
    let values = values
        .iter()
        .map(|value| number_from_value(&mut cx, value))
        .collect::<Vec<_>>();
    assert_eq!(values, vec![7, 8]);
}

#[test]
fn sequence_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_sequence_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(sequence_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(sequence_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "sequence organ ops").unwrap();

    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("sequence", "transduce.v1"))
    }));
}
