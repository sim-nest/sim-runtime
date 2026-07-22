use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// conformance: runtime organs prove facts through the logic library.

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, NumberLiteral, ShapeMatch, Symbol};

use crate::{
    LogicConfig, LogicDb,
    all_solutions::{FindallRequest, findall_through_sequence_with_probe},
    env::LogicEnv,
    query::query_all,
};

fn cx_with_number_tower() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    cx.load_lib(&sim_lib_numbers_arith::NumbersArithmeticLib::new())
        .unwrap();
    cx.load_lib(&sim_lib_numbers_i64::I64NumbersLib::new())
        .unwrap();
    cx.load_lib(&sim_lib_numbers_f64::F64NumbersLib::new())
        .unwrap();
    cx.load_lib(&sim_lib_numbers_bigint::BigIntNumbersLib::new())
        .unwrap();
    cx
}

fn number(domain: &str, canonical: impl Into<String>) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.into(),
    })
}

fn capture<'a>(answer: &'a ShapeMatch, name: &str) -> &'a Expr {
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(symbol, expr)| (symbol == &Symbol::new(name)).then_some(expr))
        .unwrap()
}

fn color_db() -> LogicDb {
    let mut db = LogicDb::new();
    for color in ["red", "green", "blue"] {
        db.assert_clause_expr(Expr::List(vec![
            Expr::Symbol(Symbol::new("fact")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("color")),
                Expr::Symbol(Symbol::new(color)),
            ]),
        ]))
        .unwrap();
    }
    db
}

#[test]
fn is_routes_mixed_terms_through_number_tower() {
    let mut cx = cx_with_number_tower();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("is")),
            Expr::Local(Symbol::new("X")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("+")),
                number("i64", "1"),
                number("f64", "0.5"),
            ]),
        ]),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(capture(&answers[0], "X"), &number("f64", "1.5"));
}

#[test]
fn is_widens_overflowing_integer_terms_through_number_tower() {
    let mut cx = cx_with_number_tower();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("is")),
            Expr::Local(Symbol::new("X")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("+")),
                number("i64", i64::MAX.to_string()),
                number("i64", "1"),
            ]),
        ]),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(
        capture(&answers[0], "X"),
        &number("bigint", "9223372036854775808")
    );
}

#[test]
fn findall_collects_answers_forced_from_sequence_engine() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let db = color_db();
    let config = LogicConfig::default();
    let forced = Arc::new(AtomicUsize::new(0));
    let probe = Arc::clone(&forced);
    let template = Expr::Local(Symbol::new("X"));
    let goal = Expr::List(vec![
        Expr::Symbol(Symbol::new("color")),
        Expr::Local(Symbol::new("X")),
    ]);
    let output = Expr::Local(Symbol::new("Xs"));
    let env = LogicEnv::new();
    let envs = findall_through_sequence_with_probe(
        &mut cx,
        FindallRequest {
            db: &db,
            config: &config,
            template: &template,
            goal: &goal,
            output: &output,
            env: &env,
        },
        |_| {
            probe.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();

    assert_eq!(forced.load(Ordering::SeqCst), 3);
    assert_eq!(envs.len(), 1);
    assert_eq!(
        envs[0].get(&Symbol::new("Xs")),
        Some(&Expr::List(vec![
            Expr::Symbol(Symbol::new("red")),
            Expr::Symbol(Symbol::new("green")),
            Expr::Symbol(Symbol::new("blue")),
        ]))
    );
}

#[test]
fn findall_query_projects_answer_template() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all(
        &mut cx,
        &color_db(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("findall")),
            Expr::Local(Symbol::new("X")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("color")),
                Expr::Local(Symbol::new("X")),
            ]),
            Expr::Local(Symbol::new("Xs")),
        ]),
        Some(1),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(
        capture(&answers[0], "Xs"),
        &Expr::List(vec![
            Expr::Symbol(Symbol::new("red")),
            Expr::Symbol(Symbol::new("green")),
            Expr::Symbol(Symbol::new("blue")),
        ])
    );
}
