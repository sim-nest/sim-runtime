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

    let exprs = values
        .into_iter()
        .map(|value| value.object().as_expr(&mut cx).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(exprs.len(), 3);
    assert!(exprs.contains(&Expr::Symbol(Symbol::qualified("sequence", "map.v1"))));
    assert!(exprs.contains(&Expr::Symbol(Symbol::qualified("sequence", "filter.v1"))));
    assert!(exprs.contains(&Expr::Symbol(Symbol::qualified("sequence", "reduce.v1"))));
}

#[test]
fn sequence_live_claims_match_loaded_exports() {
    let mut cx = cx();
    install_sequence_lib(&mut cx).unwrap();
    let lib = cx.registry().lib(&manifest_name()).unwrap().clone();
    publish_sequence_organ_claims_for_lib(&mut cx, lib.id).unwrap();

    let card = card_for_ref(&mut cx, Ref::Symbol(sequence_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let card_ops = force_list_to_vec(&mut cx, list, "sequence live ops")
        .unwrap()
        .into_iter()
        .map(|value| value.object().as_expr(&mut cx).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(card_ops.len(), sequence_live_ops().len());

    for (op_key, export_symbol) in sequence_live_ops() {
        let op_symbol = Symbol::qualified(
            op_key.namespace.to_string(),
            format!("{}.v{}", op_key.name, op_key.version),
        );
        assert!(
            card_ops.contains(&Expr::Symbol(op_symbol.clone())),
            "missing live sequence claim {op_symbol}"
        );
        assert!(
            lib.exports
                .iter()
                .any(|export| export.symbol == export_symbol),
            "missing sequence export {export_symbol}"
        );
        assert!(
            cx.resolve_function(&export_symbol).is_ok(),
            "{export_symbol}"
        );
    }
}

// ---- COOKBOOK_7 COOK7.02: the sequence organ (seq/map|filter|fold) ----

type TestFnBody = Arc<
    dyn Fn(&mut Cx, Vec<sim_kernel::Value>) -> sim_kernel::Result<sim_kernel::Value> + Send + Sync,
>;

/// A closure-backed callable for exercising the higher-order sequence ops.
#[derive(Clone)]
struct TestFn(TestFnBody);

impl sim_kernel::Object for TestFn {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<test-fn>".to_owned())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for TestFn {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<sim_kernel::ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn sim_kernel::Callable> {
        Some(self)
    }
}

impl sim_kernel::Callable for TestFn {
    fn call(&self, cx: &mut Cx, args: sim_kernel::Args) -> sim_kernel::Result<sim_kernel::Value> {
        (self.0)(cx, args.into_vec())
    }
}

fn test_fn(cx: &mut Cx, body: TestFnBody) -> sim_kernel::Value {
    cx.factory().opaque(Arc::new(TestFn(body))).unwrap()
}

fn string(cx: &mut Cx, text: &str) -> sim_kernel::Value {
    cx.factory().string(text.to_owned()).unwrap()
}

fn string_of(cx: &mut Cx, value: &sim_kernel::Value) -> String {
    let Expr::String(text) = value.object().as_expr(cx).unwrap() else {
        panic!("expected string value");
    };
    text
}

fn strings_of(cx: &mut Cx, value: &sim_kernel::Value) -> Vec<String> {
    let Expr::List(items) = value.object().as_expr(cx).unwrap() else {
        panic!("expected list value");
    };
    items
        .into_iter()
        .map(|item| {
            let Expr::String(text) = item else {
                panic!("expected string element");
            };
            text
        })
        .collect()
}

#[test]
fn seq_map_filter_fold_apply_a_function_over_a_list() {
    use sim_kernel::Callable as _;

    let mut cx = cx();
    install_sequence_lib(&mut cx).unwrap();

    // The ops are installed as `seq/*` callables.
    for op in SeqOp::ALL {
        assert!(cx.resolve_function(&op.symbol()).is_ok(), "{}", op.symbol());
    }

    // map: append "!" to every element.
    let bang = test_fn(
        &mut cx,
        Arc::new(|cx, args| {
            let s = string_of(cx, &args[0]);
            Ok(string(cx, &format!("{s}!")))
        }),
    );
    let a = string(&mut cx, "a");
    let b = string(&mut cx, "b");
    let list = cx.factory().list(vec![a, b]).unwrap();
    let mapped = SequenceFunction::new(SeqOp::Map)
        .call(&mut cx, sim_kernel::Args::new(vec![bang, list]))
        .unwrap();
    assert_eq!(strings_of(&mut cx, &mapped), vec!["a!", "b!"]);

    // filter: keep elements equal to "keep".
    let is_keep = test_fn(
        &mut cx,
        Arc::new(|cx, args| {
            let keep = string_of(cx, &args[0]) == "keep";
            cx.factory().bool(keep)
        }),
    );
    let k1 = string(&mut cx, "keep");
    let d = string(&mut cx, "drop");
    let k2 = string(&mut cx, "keep");
    let list = cx.factory().list(vec![k1, d, k2]).unwrap();
    let filtered = SequenceFunction::new(SeqOp::Filter)
        .call(&mut cx, sim_kernel::Args::new(vec![is_keep, list]))
        .unwrap();
    assert_eq!(strings_of(&mut cx, &filtered), vec!["keep", "keep"]);

    // fold: concatenate left to right from an empty seed.
    let concat = test_fn(
        &mut cx,
        Arc::new(|cx, args| {
            let acc = string_of(cx, &args[0]);
            let item = string_of(cx, &args[1]);
            Ok(string(cx, &format!("{acc}{item}")))
        }),
    );
    let seed = string(&mut cx, "");
    let fa = string(&mut cx, "a");
    let fb = string(&mut cx, "b");
    let fc = string(&mut cx, "c");
    let list = cx.factory().list(vec![fa, fb, fc]).unwrap();
    let folded = SequenceFunction::new(SeqOp::Fold)
        .call(&mut cx, sim_kernel::Args::new(vec![concat, seed, list]))
        .unwrap();
    assert_eq!(string_of(&mut cx, &folded), "abc");

    // Arity is checked.
    let err = SequenceFunction::new(SeqOp::Map)
        .call(&mut cx, sim_kernel::Args::new(vec![]))
        .unwrap_err();
    assert!(matches!(err, sim_kernel::Error::Eval(msg) if msg.contains("expects")));
}
