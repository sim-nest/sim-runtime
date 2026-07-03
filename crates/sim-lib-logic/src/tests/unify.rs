use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};

use crate::{LogicConfig, LogicEnv, model::OccursCheck, unify::unify_exprs};

#[test]
fn unify_binds_repeated_variables_across_lists() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let left = Expr::List(vec![
        Expr::Symbol(Symbol::new("pair")),
        Expr::Local(Symbol::new("x")),
        Expr::Local(Symbol::new("x")),
    ]);
    let right = Expr::List(vec![
        Expr::Symbol(Symbol::new("pair")),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
    ]);
    let matched = unify_exprs(&mut cx, &LogicConfig::default(), &left, &right).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 1);
}

#[test]
fn occurs_check_rejects_cycles() {
    let mut env = LogicEnv::new();
    let value = Expr::List(vec![
        Expr::Symbol(Symbol::new("loop")),
        Expr::Local(Symbol::new("x")),
    ]);
    let err = env
        .bind(Symbol::new("x"), value, OccursCheck::Always)
        .unwrap_err();
    assert!(format!("{err}").contains("occurs check"));
}

#[test]
fn shape_unify_binds_logic_variable() {
    let mut env = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Local(Symbol::new("X")),
    ]);
    let subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(env.unify(&pattern, &subject, OccursCheck::Always).unwrap());
    assert_eq!(
        env.get(&Symbol::new("X")),
        Some(&Expr::Symbol(Symbol::new("bob")))
    );
}

#[test]
fn shape_unify_fails_on_mismatch() {
    let mut env = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Local(Symbol::new("X")),
    ]);
    let subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("child")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(!env.unify(&pattern, &subject, OccursCheck::Always).unwrap());
    assert_eq!(env.get(&Symbol::new("X")), None);
}

#[test]
fn shape_unify_repeated_variable_requires_same_subject() {
    let mut accepted = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Local(Symbol::new("X")),
        Expr::Local(Symbol::new("X")),
    ]);
    let same_subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Symbol(Symbol::new("bob")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(
        accepted
            .unify(&pattern, &same_subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(
        accepted.get(&Symbol::new("X")),
        Some(&Expr::Symbol(Symbol::new("bob")))
    );

    let mut rejected = LogicEnv::new();
    let different_subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Symbol(Symbol::new("bob")),
        Expr::Symbol(Symbol::new("alice")),
    ]);
    assert!(
        !rejected
            .unify(&pattern, &different_subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(rejected.get(&Symbol::new("X")), None);
}

#[test]
fn unify_returns_false_on_mismatch() {
    let mut env = LogicEnv::new();
    let accepted = env
        .unify(
            &Expr::Symbol(Symbol::new("a")),
            &Expr::Symbol(Symbol::new("b")),
            OccursCheck::Always,
        )
        .unwrap();
    assert!(!accepted);
}
