use std::sync::Arc;

use sim_kernel::{
    Cx, DefaultFactory, EagerPolicy, Expr, Symbol, capability::control_prompt_capability,
};

use crate::{LogicConfig, LogicDb, query::query_all};

fn test_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_lib_control::install_control_policy(&mut cx);
    cx.grant(control_prompt_capability());
    cx
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

fn rule(head: Expr, body: Vec<Expr>) -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        head,
        Expr::List(body),
    ])
}

fn cut() -> Expr {
    Expr::Symbol(Symbol::new("!"))
}

fn symbol_expr(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

fn values_for(answers: &[sim_kernel::ShapeMatch], name: &str) -> Vec<Expr> {
    let symbol = Symbol::new(name);
    answers
        .iter()
        .filter_map(|answer| {
            answer
                .captures
                .exprs()
                .iter()
                .find_map(|(captured, expr)| (captured == &symbol).then(|| expr.clone()))
        })
        .collect()
}

#[test]
fn green_cut_keeps_first_matching_clause() {
    let mut cx = test_cx();
    let mut db = LogicDb::new();
    db.assert_clause_expr(fact(call("choice", vec![symbol_expr("first")])))
        .unwrap();
    db.assert_clause_expr(fact(call("choice", vec![symbol_expr("second")])))
        .unwrap();
    db.assert_clause_expr(rule(
        call("first-choice", vec![local("x")]),
        vec![call("choice", vec![local("x")]), cut()],
    ))
    .unwrap();

    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        call("first-choice", vec![local("who")]),
        Some(10),
    )
    .unwrap();

    assert_eq!(values_for(&answers, "who"), vec![symbol_expr("first")]);
}

#[test]
fn red_cut_stops_later_clauses_for_predicate() {
    let mut cx = test_cx();
    let mut db = LogicDb::new();
    db.assert_clause_expr(fact(call("gate", vec![symbol_expr("open")])))
        .unwrap();
    db.assert_clause_expr(rule(
        call("pick", vec![local("x")]),
        vec![
            call("gate", vec![symbol_expr("open")]),
            cut(),
            call("=", vec![local("x"), symbol_expr("red")]),
        ],
    ))
    .unwrap();
    db.assert_clause_expr(fact(call("pick", vec![symbol_expr("blue")])))
        .unwrap();

    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        call("pick", vec![local("who")]),
        Some(10),
    )
    .unwrap();

    assert_eq!(values_for(&answers, "who"), vec![symbol_expr("red")]);
}

#[test]
fn cut_inside_rule_body_leaves_outer_continuations() {
    let mut cx = test_cx();
    let mut db = LogicDb::new();
    db.assert_clause_expr(rule(
        call("entry", vec![local("branch"), local("color")]),
        vec![call("chosen", vec![local("branch"), local("color")])],
    ))
    .unwrap();
    db.assert_clause_expr(fact(call(
        "entry",
        vec![symbol_expr("fallback"), symbol_expr("none")],
    )))
    .unwrap();
    db.assert_clause_expr(fact(call("branch", vec![symbol_expr("left")])))
        .unwrap();
    db.assert_clause_expr(fact(call("branch", vec![symbol_expr("right")])))
        .unwrap();
    db.assert_clause_expr(fact(call("color", vec![symbol_expr("red")])))
        .unwrap();
    db.assert_clause_expr(fact(call("color", vec![symbol_expr("blue")])))
        .unwrap();
    db.assert_clause_expr(rule(
        call("chosen", vec![local("branch"), local("color")]),
        vec![
            call("branch", vec![local("branch")]),
            call("color", vec![local("color")]),
            cut(),
        ],
    ))
    .unwrap();

    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        call("entry", vec![local("branch"), local("color")]),
        Some(10),
    )
    .unwrap();

    assert_eq!(
        values_for(&answers, "branch"),
        vec![symbol_expr("left"), symbol_expr("fallback")]
    );
    assert_eq!(
        values_for(&answers, "color"),
        vec![symbol_expr("red"), symbol_expr("none")]
    );
}
