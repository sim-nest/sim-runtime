use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, NumberLiteral, ShapeMatch, Symbol};

use crate::{LogicConfig, LogicDb, builtins::BuiltinTable, query::query_all};

fn cx_with_number_tower() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    cx.load_lib(&sim_lib_numbers_arith::NumbersArithmeticLib::new())
        .unwrap();
    cx.load_lib(&sim_lib_numbers_i64::I64NumbersLib::new())
        .unwrap();
    cx.load_lib(&sim_lib_numbers_f64::F64NumbersLib::new())
        .unwrap();
    cx
}

fn number(domain: &str, canonical: impl Into<String>) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.into(),
    })
}

fn query_compare(op: &str, left: Expr, right: Expr) -> Vec<ShapeMatch> {
    let mut cx = cx_with_number_tower();
    query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![Expr::Symbol(Symbol::new(op)), left, right]),
        Some(2),
    )
    .unwrap()
}

#[test]
fn comparison_family_is_registered_as_number_arith() {
    let table = BuiltinTable::standard();

    for key in ["=:=", "=\\=", "<", "=<", ">", ">="] {
        assert_eq!(
            table.organ_of(&Symbol::new(key)),
            Some(&Symbol::qualified("numbers", "arith"))
        );
    }
}

#[test]
fn cross_domain_equality_promotes_through_number_tower() {
    let answers = query_compare("=:=", number("i64", "2"), number("f64", "2.0"));

    assert_eq!(answers.len(), 1);
}

#[test]
fn arithmetic_comparison_family_obeys_cmp_result() {
    let true_cases = [
        ("=\\=", number("i64", "2"), number("i64", "3")),
        ("<", number("i64", "1"), number("i64", "2")),
        ("=<", number("i64", "2"), number("i64", "2")),
        (">", number("i64", "3"), number("i64", "2")),
        (">=", number("i64", "2"), number("i64", "2")),
    ];
    for (op, left, right) in true_cases {
        assert_eq!(query_compare(op, left, right).len(), 1, "{op} should hold");
    }

    assert!(query_compare("<", number("i64", "3"), number("i64", "2")).is_empty());
}
