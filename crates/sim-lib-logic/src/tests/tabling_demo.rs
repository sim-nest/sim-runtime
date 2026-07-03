use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ShapeMatch, Symbol};

use crate::{
    LogicConfig, LogicDb, LogicLimits,
    builtins::{BuiltinTable, tabling_memo_binding},
    query::{query_all, query_all_with_builtins},
};

#[test]
fn tabling_is_just_a_registry_entry_no_resolver_change() {
    let db = left_recursive_path_db();
    let config = config_with_depth(16);
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));

    let plain = query_all(
        &mut cx,
        &db,
        &config,
        goal("path", vec![symbol("a"), local("Y")]),
        Some(8),
    );
    assert!(
        plain.is_err(),
        "plain left-recursive path/2 should hit the existing resolver guard"
    );

    let mut table = BuiltinTable::standard();
    table.register(tabling_memo_binding(Symbol::new("path")));

    let answers = query_all_with_builtins(
        &mut cx,
        &db,
        &config,
        goal("path", vec![symbol("a"), local("Y")]),
        Some(8),
        table,
    )
    .unwrap();
    let ys = answer_labels(&answers, "Y");

    assert_eq!(ys, vec!["b", "c"]);
}

fn left_recursive_path_db() -> LogicDb {
    let mut db = LogicDb::new();
    db.assert_clause_expr(rule(
        goal("path", vec![local("X"), local("Y")]),
        vec![
            goal("path", vec![local("X"), local("Z")]),
            goal("edge", vec![local("Z"), local("Y")]),
        ],
    ))
    .unwrap();
    db.assert_clause_expr(rule(
        goal("path", vec![local("X"), local("Y")]),
        vec![goal("edge", vec![local("X"), local("Y")])],
    ))
    .unwrap();
    db.assert_clause_expr(fact("edge", vec![symbol("a"), symbol("b")]))
        .unwrap();
    db.assert_clause_expr(fact("edge", vec![symbol("b"), symbol("c")]))
        .unwrap();
    db
}

fn config_with_depth(max_depth: usize) -> LogicConfig {
    LogicConfig {
        limits: LogicLimits {
            max_depth,
            ..LogicLimits::default()
        },
        ..LogicConfig::default()
    }
}

fn answer_labels(answers: &[ShapeMatch], name: &str) -> Vec<String> {
    answers
        .iter()
        .filter_map(|answer| capture(answer, name))
        .map(expr_label)
        .collect()
}

fn capture<'a>(answer: &'a ShapeMatch, name: &str) -> Option<&'a Expr> {
    let name = Symbol::new(name);
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(symbol, expr)| (symbol == &name).then_some(expr))
}

fn rule(head: Expr, body: Vec<Expr>) -> Expr {
    Expr::List(vec![symbol("rule"), head, Expr::List(body)])
}

fn fact(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(vec![symbol("fact"), goal(name, args)])
}

fn goal(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(std::iter::once(symbol(name)).chain(args).collect())
}

fn symbol(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

fn expr_label(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(symbol) => symbol.to_string(),
        other => format!("{other:?}"),
    }
}
