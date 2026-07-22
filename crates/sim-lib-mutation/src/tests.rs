use std::sync::Arc;

use sim_kernel::{
    Cx, Error, Expr, Object, ObjectCompat, Ref, Symbol, Table, Value,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};
use sim_lib_sequence::{
    RuntimeIndexSource, force_sequence_bounded, persistent_vector, runtime_index_sequence,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn int(cx: &mut Cx, value: i64) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "i64"), value.to_string())
        .unwrap()
}

fn float(cx: &mut Cx, value: f64) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "f64"), value.to_string())
        .unwrap()
}

fn bool_value(cx: &mut Cx, value: bool) -> Value {
    cx.factory().bool(value).unwrap()
}

fn expr(cx: &mut Cx, value: &Value) -> Expr {
    value.object().as_expr(cx).unwrap()
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
fn runtime_table_accepts_dict_keys_and_projects_array() {
    let mut cx = cx();
    let table = Arc::new(MutableRuntimeTable::new(DemoPolicy));

    let denied_key = int(&mut cx, 1);
    let denied_value = string(&mut cx, "denied");
    assert!(matches!(
        table.set(&mut cx, denied_key, denied_value).unwrap_err(),
        Error::CapabilityDenied { .. }
    ));

    cx.grant(standard_mutate_capability());
    let one_key = int(&mut cx, 1);
    let one = string(&mut cx, "one");
    table.set(&mut cx, one_key, one).unwrap();
    let two_key = int(&mut cx, 2);
    let two = string(&mut cx, "two");
    table.set(&mut cx, two_key, two).unwrap();
    let gap_key = int(&mut cx, 4);
    let gap = string(&mut cx, "after-gap");
    table.set(&mut cx, gap_key, gap).unwrap();

    let str_key = string(&mut cx, "name");
    let str_value = string(&mut cx, "dict");
    table.set(&mut cx, str_key.clone(), str_value).unwrap();
    let bool_key = bool_value(&mut cx, true);
    let bool_value = string(&mut cx, "truth");
    table.set(&mut cx, bool_key, bool_value).unwrap();
    let float_key = float(&mut cx, 1.5);
    let float_value = string(&mut cx, "float");
    table.set(&mut cx, float_key, float_value).unwrap();
    let object_key = cx.factory().opaque(Arc::new(TestObject("id"))).unwrap();
    let object_value = string(&mut cx, "object");
    table
        .set(&mut cx, object_key.clone(), object_value)
        .unwrap();

    let str_entry = table.get(&mut cx, &str_key).unwrap().unwrap();
    assert_eq!(expr(&mut cx, &str_entry), Expr::String("dict".to_owned()));
    let object_runtime_key = RuntimeKey::from_value(&mut cx, &object_key)
        .unwrap()
        .unwrap();
    assert!(matches!(object_runtime_key, RuntimeKey::ObjectIdentity(_)));
    assert_eq!(
        expr(
            &mut cx,
            &table
                .get_runtime_key(&object_runtime_key)
                .unwrap()
                .expect("object identity entry")
        ),
        Expr::String("object".to_owned())
    );

    let keys = table
        .entries_in_key_order()
        .unwrap()
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    assert!(keys.contains(&RuntimeKey::Bool(true)));
    assert!(keys.contains(&RuntimeKey::Integer(1)));
    assert!(keys.contains(&RuntimeKey::Str("name".to_owned())));
    assert!(keys.contains(&RuntimeKey::FloatBits(1.5f64.to_bits())));

    let sequence = runtime_index_sequence(&mut cx, table.clone(), 1).unwrap();
    let values = force_sequence_bounded(&mut cx, &sequence, 8, "runtime table projection").unwrap();
    let values = values
        .iter()
        .map(|value| expr(&mut cx, value))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            Expr::String("one".to_owned()),
            Expr::String("two".to_owned())
        ]
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

#[derive(Clone, Copy)]
struct DemoPolicy;

impl RuntimeKeyPolicy for DemoPolicy {
    fn key_for(&self, cx: &mut Cx, value: &Value) -> sim_kernel::Result<Option<RuntimeKey>> {
        RuntimeKey::from_value(cx, value)
    }
}

impl RuntimeIndexSource for MutableRuntimeTable<DemoPolicy> {
    fn value_at_runtime_index(
        &self,
        _cx: &mut Cx,
        index: i64,
    ) -> sim_kernel::Result<Option<Value>> {
        self.get_runtime_key(&RuntimeKey::Integer(index))
    }
}

struct TestObject(&'static str);

impl Object for TestObject {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok(format!("#<test-object {}>", self.0))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TestObject {}
