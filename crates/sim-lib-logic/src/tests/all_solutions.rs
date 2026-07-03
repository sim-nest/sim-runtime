use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ShapeMatch, Symbol};

use crate::{LogicConfig, LogicDb, builtins::BuiltinTable, query::query_all};

fn test_cx() -> Cx {
    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}

fn capture<'a>(answer: &'a ShapeMatch, name: &str) -> &'a Expr {
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(symbol, expr)| (symbol == &Symbol::new(name)).then_some(expr))
        .unwrap()
}

fn symbol(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

fn list(items: Vec<Expr>) -> Expr {
    Expr::List(items)
}

fn goal(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(std::iter::once(symbol(name)).chain(args).collect())
}

fn existential(var: &str, goal: Expr) -> Expr {
    Expr::List(vec![symbol("^"), local(var), goal])
}

fn fact(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(vec![
        symbol("fact"),
        Expr::List(std::iter::once(symbol(name)).chain(args).collect()),
    ])
}

fn family_db() -> LogicDb {
    let mut db = LogicDb::new();
    for (parent, child) in [
        ("alice", "bob"),
        ("alice", "bea"),
        ("cara", "drew"),
        ("dana", "bob"),
    ] {
        db.assert_clause_expr(fact("parent", vec![symbol(parent), symbol(child)]))
            .unwrap();
    }
    db
}

#[test]
fn all_solution_family_is_registered_as_sequence() {
    let table = BuiltinTable::standard();

    for key in ["findall", "bagof", "setof"] {
        assert_eq!(
            table.organ_of(&Symbol::new(key)),
            Some(&Symbol::new("sequence"))
        );
    }
}

#[test]
fn findall_preserves_duplicate_template_values() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "findall",
            vec![
                local("X"),
                goal(
                    "member",
                    vec![
                        local("X"),
                        list(vec![symbol("a"), symbol("b"), symbol("a")]),
                    ],
                ),
                local("Xs"),
            ],
        ),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(
        capture(&answers[0], "Xs"),
        &list(vec![symbol("a"), symbol("b"), symbol("a")])
    );
}

#[test]
fn bagof_groups_by_free_goal_variables() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &family_db(),
        &LogicConfig::default(),
        goal(
            "bagof",
            vec![
                local("Child"),
                goal("parent", vec![local("Parent"), local("Child")]),
                local("Children"),
            ],
        ),
        Some(8),
    )
    .unwrap();

    let groups = answers
        .iter()
        .map(|answer| {
            (
                capture(answer, "Parent").clone(),
                capture(answer, "Children").clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        groups,
        vec![
            (symbol("alice"), list(vec![symbol("bob"), symbol("bea")])),
            (symbol("cara"), list(vec![symbol("drew")])),
            (symbol("dana"), list(vec![symbol("bob")])),
        ]
    );
}

#[test]
fn setof_sorts_and_deduplicates_template_values() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "setof",
            vec![
                local("X"),
                goal(
                    "member",
                    vec![
                        local("X"),
                        list(vec![symbol("c"), symbol("a"), symbol("b"), symbol("a")]),
                    ],
                ),
                local("Xs"),
            ],
        ),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(
        capture(&answers[0], "Xs"),
        &list(vec![symbol("a"), symbol("b"), symbol("c")])
    );
}

#[test]
fn bagof_fails_when_goal_has_no_solutions() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "bagof",
            vec![
                local("X"),
                goal("member", vec![local("X"), list(Vec::new())]),
                local("Xs"),
            ],
        ),
        Some(1),
    )
    .unwrap();

    assert!(answers.is_empty());
}

#[test]
fn existential_qualifier_excludes_witness_variable() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &family_db(),
        &LogicConfig::default(),
        goal(
            "bagof",
            vec![
                local("Child"),
                existential(
                    "Parent",
                    goal("parent", vec![local("Parent"), local("Child")]),
                ),
                local("Children"),
            ],
        ),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(
        capture(&answers[0], "Children"),
        &list(vec![
            symbol("bob"),
            symbol("bea"),
            symbol("drew"),
            symbol("bob"),
        ])
    );
}
