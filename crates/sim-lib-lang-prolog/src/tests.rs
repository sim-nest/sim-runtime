use std::sync::Arc;

use sim_kernel::{
    Cx, DefaultFactory, EagerPolicy, Expr, NumberLiteral, QuoteMode, ShapeMatchObject, Stream,
    Symbol, Value, capability::control_prompt_capability, logic_db_write_capability,
};

use crate::{install_prolog_lib, prolog_exports};

fn quote(expr: Expr) -> Expr {
    Expr::Quote {
        mode: QuoteMode::Quote,
        expr: Box::new(expr),
    }
}

fn symbol(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

fn number(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: text.to_owned(),
    })
}

fn prolog_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_lib_control::install_control_policy(&mut cx);
    install_prolog_lib(&mut cx).unwrap();
    cx.grant(logic_db_write_capability());
    cx.grant(control_prompt_capability());
    cx
}

fn fact(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(
            std::iter::once(Expr::Symbol(Symbol::new(name)))
                .chain(args)
                .collect(),
        ),
    ])
}

fn rule(head: Expr, body: Vec<Expr>) -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("rule")),
        head,
        Expr::List(body),
    ])
}

fn goal(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(
        std::iter::once(Expr::Symbol(Symbol::new(name)))
            .chain(args)
            .collect(),
    )
}

fn cut() -> Expr {
    Expr::Symbol(Symbol::new("!"))
}

fn naf(goal_expr: Expr) -> Expr {
    goal("not", vec![goal_expr])
}

fn assert_clause(cx: &mut Cx, clause: Expr) {
    let assert_fn = cx
        .resolve_function(&Symbol::qualified("prolog", "assert!"))
        .unwrap();
    cx.call_exprs(assert_fn, vec![quote(clause)]).unwrap();
}

fn query_all(cx: &mut Cx, goal_expr: Expr, limit: usize) -> Vec<Value> {
    query_all_result(cx, goal_expr, limit)
        .unwrap()
        .object()
        .as_list()
        .unwrap()
        .to_vec(cx, Some(limit))
        .unwrap()
}

fn query_all_result(cx: &mut Cx, goal_expr: Expr, limit: usize) -> sim_kernel::Result<Value> {
    let query_all_fn = cx
        .resolve_function(&Symbol::qualified("prolog", "query/all"))
        .unwrap();
    cx.call_exprs(
        query_all_fn,
        vec![
            quote(goal_expr),
            Expr::Symbol(Symbol::new(":limit")),
            number(&limit.to_string()),
        ],
    )
}

fn binding_expr(answer: &Value, name: &str) -> Option<Expr> {
    let symbol = Symbol::new(name);
    answer
        .object()
        .downcast_ref::<ShapeMatchObject>()
        .and_then(|matched| {
            matched
                .matched()
                .captures
                .exprs()
                .iter()
                .find_map(|(captured, expr)| (captured == &symbol).then(|| expr.clone()))
        })
}

fn bindings_for(answers: &[Value], name: &str) -> Vec<Expr> {
    answers
        .iter()
        .filter_map(|answer| binding_expr(answer, name))
        .collect()
}

#[test]
fn install_assert_and_query_all_share_logic_policy_database() {
    let mut cx = prolog_cx();
    assert_eq!(cx.eval_policy_name(), "logic");

    let assert_fn = cx
        .resolve_function(&Symbol::qualified("prolog", "assert!"))
        .unwrap();
    cx.call_exprs(
        assert_fn.clone(),
        vec![quote(fact("parent", vec![symbol("alice"), symbol("bob")]))],
    )
    .unwrap();
    cx.call_exprs(
        assert_fn,
        vec![quote(fact(
            "parent",
            vec![symbol("alice"), symbol("carol")],
        ))],
    )
    .unwrap();

    let query_all_fn = cx
        .resolve_function(&Symbol::qualified("prolog", "query/all"))
        .unwrap();
    let answers = cx
        .call_exprs(
            query_all_fn,
            vec![
                quote(goal("parent", vec![symbol("alice"), local("who")])),
                Expr::Symbol(Symbol::new(":limit")),
                number("4"),
            ],
        )
        .unwrap();
    let expr = answers.object().as_expr(&mut cx).unwrap();
    match expr {
        Expr::List(items) => assert_eq!(items.len(), 2),
        other => panic!("expected answer list, found {other:?}"),
    }

    let direct = cx
        .eval_expr(goal("parent", vec![symbol("alice"), local("who")]))
        .unwrap();
    assert!(!matches!(
        direct.object().as_expr(&mut cx).unwrap(),
        Expr::Nil
    ));
}

#[test]
fn export_records_include_prolog_query_all() {
    let exports = prolog_exports();
    assert!(
        exports
            .iter()
            .any(|record| record.symbol == Symbol::qualified("prolog", "query/all"))
    );
}

#[test]
fn prolog_conformance_pack() {
    let mut cx = prolog_cx();

    assert_clause(&mut cx, fact("color", vec![symbol("red")]));
    assert_clause(&mut cx, fact("color", vec![symbol("green")]));
    assert_clause(&mut cx, fact("color", vec![symbol("blue")]));
    let colors = query_all(&mut cx, goal("color", vec![local("x")]), 8);
    assert_eq!(colors.len(), 3, "facts and multi-clause resolution");
    assert_eq!(
        bindings_for(&colors, "x"),
        vec![symbol("red"), symbol("green"), symbol("blue")]
    );

    assert_clause(
        &mut cx,
        rule(
            goal("painted", vec![local("x")]),
            vec![goal("color", vec![local("x")])],
        ),
    );
    let painted = query_all(&mut cx, goal("painted", vec![local("shade")]), 8);
    assert_eq!(painted.len(), 3, "rule resolution");

    assert_clause(
        &mut cx,
        rule(
            goal("first-color", vec![local("x")]),
            vec![goal("color", vec![local("x")]), cut()],
        ),
    );
    let green_cut = query_all(&mut cx, goal("first-color", vec![local("shade")]), 8);
    assert_eq!(bindings_for(&green_cut, "shade"), vec![symbol("red")]);

    assert_clause(&mut cx, fact("gate", vec![symbol("open")]));
    assert_clause(
        &mut cx,
        rule(
            goal("pick", vec![local("x")]),
            vec![
                goal("gate", vec![symbol("open")]),
                cut(),
                goal("=", vec![local("x"), symbol("red")]),
            ],
        ),
    );
    assert_clause(&mut cx, fact("pick", vec![symbol("blue")]));
    let red_cut = query_all(&mut cx, goal("pick", vec![local("choice")]), 8);
    assert_eq!(bindings_for(&red_cut, "choice"), vec![symbol("red")]);

    assert_clause(
        &mut cx,
        fact("contains", vec![symbol("a"), symbol("present")]),
    );
    let not_missing = query_all(
        &mut cx,
        naf(goal("contains", vec![symbol("d"), symbol("present")])),
        1,
    );
    assert_eq!(not_missing.len(), 1, "NAF succeeds when the goal fails");
    let not_present = query_all(
        &mut cx,
        naf(goal("contains", vec![symbol("a"), symbol("present")])),
        1,
    );
    assert!(not_present.is_empty(), "NAF fails when the goal succeeds");
    let flounder = query_all_result(
        &mut cx,
        naf(goal("contains", vec![local("x"), symbol("present")])),
        1,
    );
    assert!(
        flounder.unwrap_err().to_string().contains("flounders"),
        "NAF with an unbound variable must flounder"
    );

    assert_clause(&mut cx, fact("nat", vec![number("0")]));
    assert_clause(
        &mut cx,
        rule(
            goal("nat", vec![goal("s", vec![local("n")])]),
            vec![goal("nat", vec![local("n")])],
        ),
    );
    let query_seq_fn = cx
        .resolve_function(&Symbol::qualified("prolog", "query-seq"))
        .unwrap();
    let nat_seq = cx
        .call_exprs(
            query_seq_fn,
            vec![
                quote(goal("nat", vec![local("x")])),
                Expr::Symbol(Symbol::new(":limit")),
                number("3"),
                Expr::Symbol(Symbol::new(":strategy")),
                symbol("bfs"),
                Expr::Symbol(Symbol::new(":buffer")),
                number("1"),
            ],
        )
        .unwrap();
    let stream = nat_seq.object().as_stream().unwrap();
    assert!(Stream::next(stream, &mut cx).unwrap().is_some());
    assert!(Stream::next(stream, &mut cx).unwrap().is_some());
    assert!(Stream::next(stream, &mut cx).unwrap().is_some());
    assert!(Stream::next(stream, &mut cx).unwrap().is_none());
}

#[test]
fn prolog_exports_all_registered() {
    let cx = prolog_cx();
    let expected = [
        "assert!",
        "retract!",
        "query",
        "query/all",
        "query-seq",
        "consult",
    ];
    let exports = prolog_exports();
    for name in expected {
        let symbol = Symbol::qualified("prolog", name);
        assert!(
            exports.iter().any(|record| record.symbol == symbol),
            "missing export record for {symbol}"
        );
        cx.resolve_function(&symbol)
            .unwrap_or_else(|err| panic!("missing runtime function {symbol}: {err}"));
    }
}
