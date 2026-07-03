use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, NumberLiteral, ShapeMatch, Symbol};

use crate::{LogicConfig, LogicDb, LogicLimits, builtins::BuiltinTable, query::query_all};

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

fn number(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: text.to_owned(),
    })
}

fn goal(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(std::iter::once(symbol(name)).chain(args).collect())
}

#[test]
fn list_family_is_registered_as_sequence() {
    let table = BuiltinTable::standard();

    for key in ["member", "append", "length", "select"] {
        assert_eq!(
            table.organ_of(&Symbol::new(key)),
            Some(&Symbol::new("sequence"))
        );
    }
}

#[test]
fn member_backtracks_via_sequence_organ() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "member",
            vec![
                local("X"),
                list(vec![symbol("a"), symbol("b"), symbol("c")]),
            ],
        ),
        Some(8),
    )
    .unwrap();

    let xs = answers
        .iter()
        .map(|answer| capture(answer, "X").clone())
        .collect::<Vec<_>>();
    assert_eq!(xs, vec![symbol("a"), symbol("b"), symbol("c")]);
}

#[test]
fn length_counts_closed_list_through_sequence() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "length",
            vec![list(vec![symbol("a"), symbol("b")]), local("N")],
        ),
        Some(2),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(capture(&answers[0], "N"), &number("2"));
}

#[test]
fn append_concatenates_and_splits_through_sequence() {
    let mut cx = test_cx();
    let concat = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "append",
            vec![
                list(vec![symbol("a")]),
                list(vec![symbol("b"), symbol("c")]),
                local("Xs"),
            ],
        ),
        Some(4),
    )
    .unwrap();
    assert_eq!(concat.len(), 1);
    assert_eq!(
        capture(&concat[0], "Xs"),
        &list(vec![symbol("a"), symbol("b"), symbol("c")])
    );

    let splits = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "append",
            vec![
                local("Prefix"),
                local("Suffix"),
                list(vec![symbol("a"), symbol("b")]),
            ],
        ),
        Some(8),
    )
    .unwrap();
    let split_pairs = splits
        .iter()
        .map(|answer| {
            (
                capture(answer, "Prefix").clone(),
                capture(answer, "Suffix").clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        split_pairs,
        vec![
            (list(vec![]), list(vec![symbol("a"), symbol("b")])),
            (list(vec![symbol("a")]), list(vec![symbol("b")])),
            (list(vec![symbol("a"), symbol("b")]), list(vec![])),
        ]
    );
}

#[test]
fn select_iterates_with_remainder() {
    let mut cx = test_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "select",
            vec![
                local("X"),
                list(vec![symbol("a"), symbol("b"), symbol("c")]),
                local("Rest"),
            ],
        ),
        Some(8),
    )
    .unwrap();

    let selections = answers
        .iter()
        .map(|answer| {
            (
                capture(answer, "X").clone(),
                capture(answer, "Rest").clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selections,
        vec![
            (symbol("a"), list(vec![symbol("b"), symbol("c")])),
            (symbol("b"), list(vec![symbol("a"), symbol("c")])),
            (symbol("c"), list(vec![symbol("a"), symbol("b")])),
        ]
    );
}

#[test]
fn member_is_bounded_by_logic_answer_limit() {
    let mut cx = test_cx();
    let config = LogicConfig {
        limits: LogicLimits {
            max_answers: Some(2),
            ..LogicLimits::default()
        },
        ..LogicConfig::default()
    };

    let err = query_all(
        &mut cx,
        &LogicDb::new(),
        &config,
        goal(
            "member",
            vec![
                local("X"),
                list(vec![symbol("a"), symbol("b"), symbol("c")]),
            ],
        ),
        None,
    )
    .unwrap_err();

    assert!(err.to_string().contains("sequence exceeds force bound 2"));
}

#[test]
fn open_list_forms_are_rejected_explicitly() {
    let mut cx = test_cx();
    let err = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        goal(
            "member",
            vec![
                local("X"),
                Expr::Extension {
                    tag: Symbol::qualified("prolog", "open-list"),
                    payload: Box::new(list(vec![symbol("a"), local("Tail")])),
                },
            ],
        ),
        Some(2),
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("expected closed Prolog list expression")
    );
}
