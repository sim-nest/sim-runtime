use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, MatchScore, ShapeMatch, Stream, Symbol};

use crate::{LogicConfig, LogicDb, SearchStrategy, query::query, stream::LogicStream};

#[test]
fn stream_next_and_close_are_stable() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let stream = LogicStream::new(
        vec![
            ShapeMatch::accept(MatchScore::exact(1)),
            ShapeMatch::accept(MatchScore::exact(2)),
        ],
        1,
    );
    assert!(Stream::next(&stream, &mut cx).unwrap().is_some());
    assert!(Stream::next(&stream, &mut cx).unwrap().is_some());
    assert!(Stream::next(&stream, &mut cx).unwrap().is_none());
    Stream::close(&stream, &mut cx).unwrap();
    Stream::close(&stream, &mut cx).unwrap();
}

#[test]
fn lazy_sequence_engine_does_not_pre_collect_beyond_force_bound() {
    let mut db = LogicDb::new();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(vec![Expr::Symbol(Symbol::new("nat")), zero()]),
    ]))
    .unwrap();
    db.assert_clause_expr(Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("nat")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("s")),
                Expr::Local(Symbol::new("n")),
            ]),
        ]),
        Expr::List(vec![Expr::List(vec![
            Expr::Symbol(Symbol::new("nat")),
            Expr::Local(Symbol::new("n")),
        ])]),
    ]))
    .unwrap();

    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let config = LogicConfig {
        strategy: SearchStrategy::Bfs,
        stream_buffer: 1,
        limits: crate::LogicLimits {
            max_answers: Some(256),
            ..Default::default()
        },
        ..Default::default()
    };
    let stream = query(
        &mut cx,
        &db,
        &config,
        Expr::List(vec![
            Expr::Symbol(Symbol::new("nat")),
            Expr::Local(Symbol::new("x")),
        ]),
    )
    .unwrap();
    let answers = stream.collect(&mut cx, Some(2)).unwrap();
    assert_eq!(answers.len(), 2, "exactly two answers forced");
}

fn zero() -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: "0".to_owned(),
    })
}
