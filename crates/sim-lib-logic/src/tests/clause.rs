use sim_kernel::{Expr, Symbol};

use crate::{ClauseId, parse_clause_expr};

#[test]
fn parse_fact_clause() {
    let clause = parse_clause_expr(
        ClauseId(1),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("fact")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Symbol(Symbol::new("alice")),
                Expr::Symbol(Symbol::new("bob")),
            ]),
        ]),
    )
    .unwrap();
    assert!(clause.body.is_empty());
}

#[test]
fn parse_rule_clause() {
    let clause = parse_clause_expr(
        ClauseId(1),
        Expr::List(vec![
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
        ]),
    )
    .unwrap();
    assert_eq!(clause.body.len(), 2);
}

#[test]
fn malformed_clause_returns_eval_error() {
    let err = parse_clause_expr(
        ClauseId(1),
        Expr::List(vec![Expr::Symbol(Symbol::new("fact"))]),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("fact expects"));
}
