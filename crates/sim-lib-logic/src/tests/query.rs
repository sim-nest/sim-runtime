use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};

use crate::{LogicConfig, LogicDb, query::query_all};

fn number(text: &str) -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: text.to_owned(),
    })
}

#[test]
fn query_facts_and_rules() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut db = LogicDb::new();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("parent")),
            Expr::Symbol(Symbol::new("alice")),
            Expr::Symbol(Symbol::new("bob")),
        ]),
    ]))
    .unwrap();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("parent")),
            Expr::Symbol(Symbol::new("bob")),
            Expr::Symbol(Symbol::new("carol")),
        ]),
    ]))
    .unwrap();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("grandparent")),
            Expr::Local(Symbol::new("x")),
            Expr::Local(Symbol::new("z")),
        ]),
        Expr::List(vec![
            Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Local(Symbol::new("x")),
                Expr::Local(Symbol::new("y")),
            ]),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Local(Symbol::new("y")),
                Expr::Local(Symbol::new("z")),
            ]),
        ]),
    ]))
    .unwrap();

    let answers = query_all(
        &mut cx,
        &db,
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("grandparent")),
            Expr::Symbol(Symbol::new("alice")),
            Expr::Local(Symbol::new("who")),
        ]),
        Some(10),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
}

#[test]
fn recursive_queries_obey_max_depth() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut db = LogicDb::new();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("loop")),
            Expr::Local(Symbol::new("x")),
        ]),
        Expr::List(vec![Expr::List(vec![
            Expr::Symbol(Symbol::new("loop")),
            Expr::Local(Symbol::new("x")),
        ])]),
    ]))
    .unwrap();
    let mut config = LogicConfig::default();
    config.limits.max_depth = 4;
    let err = query_all(
        &mut cx,
        &db,
        &config,
        Expr::List(vec![Expr::Symbol(Symbol::new("loop")), number("1")]),
        Some(1),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("max_depth"));
}
