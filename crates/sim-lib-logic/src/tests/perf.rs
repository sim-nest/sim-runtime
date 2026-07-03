// Perf configs are built field-by-field for readability; the Default-then-assign
// form keeps each tuned knob on its own line.
#![allow(clippy::field_reassign_with_default)]

use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};

use crate::{LogicConfig, LogicDb, SearchStrategy, query::query_all};

fn sample_db() -> LogicDb {
    let mut db = LogicDb::new();
    for (parent, child) in [("alice", "bob"), ("alice", "carol"), ("bob", "dan")] {
        db.assert_clause_expr(Expr::List(vec![
            Expr::Symbol(Symbol::new("fact")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Symbol(Symbol::new(parent)),
                Expr::Symbol(Symbol::new(child)),
            ]),
        ]))
        .unwrap();
    }
    db
}

#[test]
fn indexed_lookup_returns_same_answers_as_linear_scan() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let db = sample_db();
    let goal = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("alice")),
        Expr::Local(Symbol::new("who")),
    ]);

    let mut linear = LogicConfig::default();
    linear.enable_indexing = false;
    let mut indexed = linear.clone();
    indexed.enable_indexing = true;

    let linear_answers = query_all(&mut cx, &db, &linear, goal.clone(), Some(10)).unwrap();
    let indexed_answers = query_all(&mut cx, &db, &indexed, goal, Some(10)).unwrap();
    assert_eq!(
        format!("{linear_answers:?}"),
        format!("{indexed_answers:?}")
    );
}

#[test]
fn fair_mode_finds_answer_hidden_behind_recursive_branch() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut db = LogicDb::new();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("path")),
            Expr::Local(Symbol::new("x")),
        ]),
        Expr::List(vec![Expr::List(vec![
            Expr::Symbol(Symbol::new("path")),
            Expr::Local(Symbol::new("x")),
        ])]),
    ]))
    .unwrap();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("path")),
            Expr::Symbol(Symbol::new("done")),
        ]),
    ]))
    .unwrap();

    let mut config = LogicConfig::default();
    config.strategy = SearchStrategy::Fair;
    config.limits.max_depth = 8;

    let answers = query_all(
        &mut cx,
        &db,
        &config,
        Expr::List(vec![
            Expr::Symbol(Symbol::new("path")),
            Expr::Local(Symbol::new("result")),
        ]),
        Some(1),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
}
