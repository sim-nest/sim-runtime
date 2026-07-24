// conformance: incremental query organ callables, cutoff, reports, and Card projection

use sim_kernel::{
    Args, Cx, Error, Expr, Ref, Symbol, Value,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn expr_value(cx: &mut Cx, expr: Expr) -> Value {
    cx.factory().expr(expr).unwrap()
}

fn call(cx: &mut Cx, name: &str, args: Vec<Value>) -> Value {
    let function = cx
        .resolve_function(&Symbol::qualified("incremental", name))
        .unwrap();
    let callable = function.object().as_callable().unwrap();
    callable.call(cx, Args::new(args)).unwrap()
}

fn call_err(cx: &mut Cx, name: &str, args: Vec<Value>) -> Error {
    let function = cx
        .resolve_function(&Symbol::qualified("incremental", name))
        .unwrap();
    let callable = function.object().as_callable().unwrap();
    callable.call(cx, Args::new(args)).unwrap_err()
}

fn register_query(cx: &mut Cx, engine: &Value, key: &str, source: Expr) -> Value {
    let key = string(cx, key);
    let source = expr_value(cx, source);
    call(cx, "register", vec![engine.clone(), key, source])
}

fn verify_query(cx: &mut Cx, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, "verify", vec![engine.clone(), key])
}

fn invalidate_key(cx: &mut Cx, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, "invalidate", vec![engine.clone(), key])
}

fn report(cx: &mut Cx, name: &str, engine: &Value) -> Value {
    call(cx, name, vec![engine.clone()])
}

fn keyed_report(cx: &mut Cx, name: &str, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, name, vec![engine.clone(), key])
}

fn table_get(cx: &mut Cx, table_value: &Value, key: &str) -> Value {
    table_value
        .object()
        .as_table_impl()
        .unwrap()
        .get(cx, Symbol::new(key))
        .unwrap()
}

fn number_text(cx: &mut Cx, value: &Value) -> String {
    match value.object().as_expr(cx).unwrap() {
        Expr::Number(number) => number.canonical,
        other => panic!("expected number, got {other:?}"),
    }
}

fn grant_all(cx: &mut Cx) {
    cx.grant(incremental_read_capability());
    cx.grant(incremental_write_capability());
    cx.grant(incremental_verify_capability());
}

fn read_expr(key: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("incremental", "read"))),
        args: vec![Expr::String(key.to_owned())],
    }
}

fn missing_expr(key: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("incremental", "missing"))),
        args: vec![Expr::String(key.to_owned())],
    }
}

#[test]
fn callables_have_shape_contracts_and_capability_gates() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    let engine = call(&mut cx, "engine", Vec::new());

    for name in [
        "engine",
        "register",
        "invalidate",
        "verify",
        "explain",
        "snapshot",
        "metrics",
    ] {
        let function = cx
            .resolve_function(&Symbol::qualified("incremental", name))
            .unwrap();
        let callable = function.object().as_callable().unwrap();
        assert!(
            callable.browse_args_shape(&mut cx).unwrap().is_some(),
            "{name}"
        );
        assert!(
            callable.browse_result_shape(&mut cx).unwrap().is_some(),
            "{name}"
        );
    }

    let key = string(&mut cx, "leaf");
    let source = expr_value(&mut cx, Expr::String("same".to_owned()));
    let denied = call_err(&mut cx, "register", vec![engine.clone(), key, source]);
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == incremental_write_capability())
    );

    cx.grant(incremental_write_capability());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));

    let key = string(&mut cx, "leaf");
    let denied = call_err(&mut cx, "verify", vec![engine, key]);
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == incremental_verify_capability())
    );
}

#[test]
fn query_family_tracks_missing_invalidation_and_cutoff() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    grant_all(&mut cx);
    let engine = call(&mut cx, "engine", Vec::new());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));
    register_query(&mut cx, &engine, "root", read_expr("leaf"));
    register_query(
        &mut cx,
        &engine,
        "guard",
        Expr::Vector(vec![read_expr("root"), missing_expr("optional")]),
    );

    let verified = verify_query(&mut cx, &engine, "guard");
    assert_eq!(
        verified.object().as_expr(&mut cx).unwrap(),
        Expr::Vector(vec![Expr::String("same".to_owned()), Expr::Bool(false)])
    );
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let leaf_count = table_get(&mut cx, &executions, "leaf");
    assert_eq!(number_text(&mut cx, &leaf_count), "1");
    let root_count = table_get(&mut cx, &executions, "root");
    assert_eq!(number_text(&mut cx, &root_count), "1");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "1");

    verify_query(&mut cx, &engine, "guard");
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "1");

    invalidate_key(&mut cx, &engine, "optional");
    verify_query(&mut cx, &engine, "guard");
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "2");
    let root_count = table_get(&mut cx, &executions, "root");
    assert_eq!(number_text(&mut cx, &root_count), "1");
}

#[test]
fn explanation_snapshot_and_card_projection_are_browseable() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    grant_all(&mut cx);
    let engine = call(&mut cx, "engine", Vec::new());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));
    register_query(&mut cx, &engine, "root", read_expr("leaf"));
    verify_query(&mut cx, &engine, "root");

    let explain = keyed_report(&mut cx, "explain", &engine, "root");
    assert_eq!(
        table_get(&mut cx, &explain, "registered")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Bool(true)
    );
    assert_ne!(
        table_get(&mut cx, &explain, "fingerprint")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );

    let snapshot = keyed_report(&mut cx, "snapshot", &engine, "root");
    let nodes = table_get(&mut cx, &snapshot, "nodes");
    let list = nodes.object().as_list().unwrap();
    let nodes = force_list_to_vec(&mut cx, list, "incremental snapshot nodes").unwrap();
    assert_eq!(nodes.len(), 2);

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(incremental_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);
    let card = card_for_ref(&mut cx, Ref::Symbol(incremental_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "incremental organ ops").unwrap();
    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("incremental", "register.v1"))
    }));
}
