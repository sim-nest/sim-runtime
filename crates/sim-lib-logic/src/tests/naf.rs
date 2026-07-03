use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};

use crate::{LogicConfig, LogicDb, query::query_all};

fn test_cx() -> Cx {
    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(Expr::Symbol(Symbol::new(name)));
    items.extend(args);
    Expr::List(items)
}

fn fact(goal: Expr) -> Expr {
    Expr::List(vec![Expr::Symbol(Symbol::new("fact")), goal])
}

fn naf(goal: Expr) -> Expr {
    call("not", vec![goal])
}

fn symbol_expr(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

#[test]
fn naf_succeeds_when_goal_fails() {
    let mut cx = test_cx();
    let db = LogicDb::new();
    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        naf(call(
            "contains",
            vec![symbol_expr("d"), symbol_expr("missing")],
        )),
        Some(1),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
}

#[test]
fn naf_fails_when_goal_succeeds() {
    let mut cx = test_cx();
    let mut db = LogicDb::new();
    db.assert_clause_expr(fact(call(
        "contains",
        vec![symbol_expr("a"), symbol_expr("present")],
    )))
    .unwrap();
    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        naf(call(
            "contains",
            vec![symbol_expr("a"), symbol_expr("present")],
        )),
        Some(1),
    )
    .unwrap();
    assert!(answers.is_empty());
}

#[test]
fn naf_flounders_on_unbound_variable() {
    let mut cx = test_cx();
    let db = LogicDb::new();
    let err = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        naf(call("foo", vec![local("x")])),
        Some(1),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("flounders"));
}
