//! Prolog matrix-row conformance runner.

use std::sync::Arc;

use sim_kernel::{
    Cx, DefaultFactory, EagerPolicy, Error, Expr, NumberLiteral, QuoteMode, Result, ShapeMatch,
    ShapeMatchObject, Symbol, Value, capability::control_prompt_capability,
    logic_db_write_capability,
};
use sim_lib_logic::{
    LogicConfig, LogicDb,
    builtins::{BuiltinTable, tabling_memo_binding},
};
use sim_lib_standard_core::{
    MatrixRunReport, MatrixRunner, SourceConformanceCase, SourceObservation,
};

use crate::conformance_all_solutions::{
    run_bagof_empty_case, run_bagof_groups_case, run_findall_duplicates_case, run_setof_sorted_case,
};
use crate::{install_prolog_lib, prolog_conformance_case_symbol, prolog_matrix_row};

/// Runs one Prolog source conformance case through the installed Prolog surface.
pub fn run_prolog_conformance_case(
    _cx: &mut Cx,
    case: &SourceConformanceCase,
) -> Result<SourceObservation> {
    if case.symbol == prolog_conformance_case_symbol("unbound-is") {
        return run_unbound_is_case();
    }
    if case.symbol == prolog_conformance_case_symbol("constraint-residual") {
        return run_constraint_residual_case();
    }
    if case.symbol == prolog_conformance_case_symbol("open-list") {
        return Ok(SourceObservation::Gap {
            code: Symbol::qualified("prolog", "open-list"),
            reason: "open and improper lists are outside the closed Expr::List bridge".to_owned(),
        });
    }
    let got = if case.symbol == prolog_conformance_case_symbol("fact") {
        run_fact_case()?
    } else if case.symbol == prolog_conformance_case_symbol("rule") {
        run_rule_case()?
    } else if case.symbol == prolog_conformance_case_symbol("query") {
        run_query_case()?
    } else if case.symbol == prolog_conformance_case_symbol("cut") {
        run_cut_case()?
    } else if case.symbol == prolog_conformance_case_symbol("is-promote") {
        run_is_promote_case()?
    } else if case.symbol == prolog_conformance_case_symbol("cmp-cross-domain") {
        run_cmp_cross_domain_case()?
    } else if case.symbol == prolog_conformance_case_symbol("cmp-false") {
        run_cmp_false_case()?
    } else if case.symbol == prolog_conformance_case_symbol("list-member") {
        run_list_member_case()?
    } else if case.symbol == prolog_conformance_case_symbol("list-append") {
        run_list_append_case()?
    } else if case.symbol == prolog_conformance_case_symbol("findall-duplicates") {
        run_findall_duplicates_case()?
    } else if case.symbol == prolog_conformance_case_symbol("bagof-groups") {
        run_bagof_groups_case()?
    } else if case.symbol == prolog_conformance_case_symbol("setof-sorted") {
        run_setof_sorted_case()?
    } else if case.symbol == prolog_conformance_case_symbol("bagof-empty") {
        run_bagof_empty_case()?
    } else if case.symbol == prolog_conformance_case_symbol("constraint-entailed") {
        run_constraint_entailed_case()?
    } else if case.symbol == prolog_conformance_case_symbol("constraint-disentailed") {
        run_constraint_disentailed_case()?
    } else if case.symbol == prolog_conformance_case_symbol("tabling-left-recursive-path") {
        run_tabling_demo_case()?
    } else {
        return Err(Error::Eval(format!(
            "unsupported prolog conformance case {}",
            case.symbol
        )));
    };
    Ok(SourceObservation::LowersTo(got))
}

/// Runs the Prolog matrix row and publishes claim-backed cells.
pub fn run_prolog_matrix_row(cx: &mut Cx) -> Result<MatrixRunReport> {
    let row = prolog_matrix_row();
    let report = MatrixRunner::run_row(cx, &row, run_prolog_conformance_case);
    report.publish_claims(cx)?;
    Ok(report)
}

fn run_fact_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    assert_clause(&mut cx, fact("color", vec![symbol("red")]))?;
    let answers = query_all(&mut cx, goal("color", vec![symbol("red")]), 4)?;
    Ok(format!("prolog:fact answers={}", answers.len()))
}

fn run_rule_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    assert_clause(&mut cx, fact("color", vec![symbol("red")]))?;
    assert_clause(
        &mut cx,
        rule(
            goal("painted", vec![local("x")]),
            vec![goal("color", vec![local("x")])],
        ),
    )?;
    let answers = query_all(&mut cx, goal("painted", vec![symbol("red")]), 4)?;
    Ok(format!("prolog:rule answers={}", answers.len()))
}

fn run_query_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    assert_clause(&mut cx, fact("color", vec![symbol("red")]))?;
    assert_clause(&mut cx, fact("color", vec![symbol("green")]))?;
    let answers = query_all(&mut cx, goal("color", vec![local("x")]), 4)?;
    Ok(format!("prolog:query answers={}", answers.len()))
}

fn run_cut_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    assert_clause(&mut cx, fact("color", vec![symbol("red")]))?;
    assert_clause(&mut cx, fact("color", vec![symbol("green")]))?;
    assert_clause(
        &mut cx,
        rule(
            goal("first-color", vec![local("x")]),
            vec![goal("color", vec![local("x")]), cut()],
        ),
    )?;
    let answers = query_all(&mut cx, goal("first-color", vec![local("shade")]), 4)?;
    let first = answers
        .first()
        .and_then(|answer| binding_expr(answer, "shade"))
        .map(|expr| expr_label(&expr))
        .unwrap_or_else(|| "none".to_owned());
    Ok(format!(
        "prolog:cut answers={} first={first}",
        answers.len()
    ))
}

fn run_is_promote_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "is",
            vec![
                local("x"),
                Expr::List(vec![symbol("+"), number("1"), number_in("f64", "0.5")]),
            ],
        ),
        4,
    )?;
    let x = answers
        .first()
        .and_then(|answer| binding_expr(answer, "x"))
        .map(|expr| expr_label(&expr))
        .unwrap_or_else(|| "none".to_owned());
    Ok(format!(
        "prolog:is organ=numbers/arith answers={} x={x}",
        answers.len()
    ))
}

fn run_cmp_cross_domain_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal("=:=", vec![number("2"), number_in("f64", "2.0")]),
        4,
    )?;
    Ok(format!(
        "prolog:compare organ=numbers/arith answers={}",
        answers.len()
    ))
}

fn run_cmp_false_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(&mut cx, goal("<", vec![number("3"), number("2")]), 4)?;
    Ok(format!("prolog:compare answers={}", answers.len()))
}

fn run_constraint_entailed_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(&mut cx, goal("#=", vec![number("2"), number("2")]), 4)?;
    Ok(format!(
        "prolog:constraint organ=control relation=#= verdict=entailed answers={}",
        answers.len()
    ))
}

fn run_constraint_disentailed_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(&mut cx, goal("#<", vec![number("3"), number("2")]), 4)?;
    Ok(format!(
        "prolog:constraint organ=control relation=#< verdict=disentailed answers={}",
        answers.len()
    ))
}

fn run_tabling_demo_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let mut table = BuiltinTable::standard();
    table.register(tabling_memo_binding(Symbol::new("path")));
    let answers = sim_lib_logic::query_all_with_builtins(
        &mut cx,
        &left_recursive_path_db()?,
        &LogicConfig::default(),
        goal("path", vec![symbol("a"), local("Y")]),
        Some(8),
        table,
    )?;
    let ys = answers
        .iter()
        .filter_map(|answer| binding_expr_from_match(answer, "Y"))
        .map(|expr| expr_label(&expr))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "prolog:tabling organ=sequence answers={} ys={ys}",
        answers.len()
    ))
}

fn run_list_member_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "member",
            vec![
                local("x"),
                list(vec![symbol("a"), symbol("b"), symbol("c")]),
            ],
        ),
        4,
    )?;
    let xs = answers
        .iter()
        .filter_map(|answer| binding_expr(answer, "x"))
        .map(|expr| expr_label(&expr))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "prolog:member organ=sequence answers={} xs={xs}",
        answers.len()
    ))
}

fn run_list_append_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "append",
            vec![
                list(vec![symbol("a")]),
                list(vec![symbol("b"), symbol("c")]),
                local("xs"),
            ],
        ),
        4,
    )?;
    let xs = answers
        .first()
        .and_then(|answer| binding_expr(answer, "xs"))
        .map(|expr| expr_label(&expr))
        .unwrap_or_else(|| "none".to_owned());
    Ok(format!(
        "prolog:append organ=sequence answers={} xs={xs}",
        answers.len()
    ))
}

fn run_unbound_is_case() -> Result<SourceObservation> {
    let mut cx = prolog_case_cx()?;
    let result = query_all(
        &mut cx,
        goal(
            "is",
            vec![
                local("x"),
                Expr::List(vec![symbol("+"), local("y"), number("1")]),
            ],
        ),
        4,
    );
    match result {
        Err(err) if err.to_string().contains("right-hand side must be ground") => {
            Ok(SourceObservation::Gap {
                code: Symbol::qualified("prolog", "unbound-arithmetic"),
                reason: "is/2 requires the right side to be ground and evaluable".to_owned(),
            })
        }
        Err(err) => Err(err),
        Ok(answers) => Err(Error::Eval(format!(
            "unbound-is unexpectedly produced {} answers",
            answers.len()
        ))),
    }
}

fn run_constraint_residual_case() -> Result<SourceObservation> {
    let mut cx = prolog_case_cx()?;
    let result = query_all(&mut cx, goal("dif", vec![local("x"), number("1")]), 4);
    match result {
        Err(err)
            if err
                .to_string()
                .contains("residual constraint demand suspended") =>
        {
            Ok(SourceObservation::Gap {
                code: Symbol::qualified("prolog", "residual-constraint"),
                reason: "residual constraint demand is suspended on the control ledger".to_owned(),
            })
        }
        Err(err) => Err(err),
        Ok(answers) => Err(Error::Eval(format!(
            "constraint-residual unexpectedly produced {} answers",
            answers.len()
        ))),
    }
}

fn left_recursive_path_db() -> Result<LogicDb> {
    let mut db = LogicDb::new();
    db.assert_clause_expr(rule(
        goal("path", vec![local("X"), local("Y")]),
        vec![
            goal("path", vec![local("X"), local("Z")]),
            goal("edge", vec![local("Z"), local("Y")]),
        ],
    ))?;
    db.assert_clause_expr(rule(
        goal("path", vec![local("X"), local("Y")]),
        vec![goal("edge", vec![local("X"), local("Y")])],
    ))?;
    db.assert_clause_expr(fact("edge", vec![symbol("a"), symbol("b")]))?;
    db.assert_clause_expr(fact("edge", vec![symbol("b"), symbol("c")]))?;
    Ok(db)
}

pub(crate) fn prolog_case_cx() -> Result<Cx> {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    cx.load_lib(&sim_lib_numbers_arith::NumbersArithmeticLib::new())?;
    cx.load_lib(&sim_lib_numbers_i64::I64NumbersLib::new())?;
    cx.load_lib(&sim_lib_numbers_f64::F64NumbersLib::new())?;
    sim_lib_control::install_control_policy(&mut cx);
    install_prolog_lib(&mut cx)?;
    cx.grant(logic_db_write_capability());
    cx.grant(control_prompt_capability());
    Ok(cx)
}

fn quote(expr: Expr) -> Expr {
    Expr::Quote {
        mode: QuoteMode::Quote,
        expr: Box::new(expr),
    }
}

pub(crate) fn symbol(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

pub(crate) fn local(name: &str) -> Expr {
    Expr::Local(Symbol::new(name))
}

fn number(text: &str) -> Expr {
    number_in("i64", text)
}

fn number_in(domain: &str, text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: text.to_owned(),
    })
}

pub(crate) fn list(items: Vec<Expr>) -> Expr {
    Expr::List(items)
}

pub(crate) fn fact(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(vec![
        symbol("fact"),
        Expr::List(std::iter::once(symbol(name)).chain(args).collect()),
    ])
}

fn rule(head: Expr, body: Vec<Expr>) -> Expr {
    Expr::List(vec![symbol("rule"), head, Expr::List(body)])
}

pub(crate) fn goal(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(std::iter::once(symbol(name)).chain(args).collect())
}

fn cut() -> Expr {
    symbol("!")
}

pub(crate) fn assert_clause(cx: &mut Cx, clause: Expr) -> Result<()> {
    let assert_fn = cx.resolve_function(&Symbol::qualified("prolog", "assert!"))?;
    cx.call_exprs(assert_fn, vec![quote(clause)])?;
    Ok(())
}

pub(crate) fn query_all(cx: &mut Cx, goal_expr: Expr, limit: usize) -> Result<Vec<Value>> {
    let query_all_fn = cx.resolve_function(&Symbol::qualified("prolog", "query/all"))?;
    cx.call_exprs(
        query_all_fn,
        vec![
            quote(goal_expr),
            Expr::Symbol(Symbol::new(":limit")),
            number(&limit.to_string()),
        ],
    )?
    .object()
    .as_list()
    .ok_or(Error::TypeMismatch {
        expected: "prolog answer list",
        found: "non-list",
    })?
    .to_vec(cx, Some(limit))
}

pub(crate) fn binding_expr(answer: &Value, name: &str) -> Option<Expr> {
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

fn binding_expr_from_match(answer: &ShapeMatch, name: &str) -> Option<Expr> {
    let symbol = Symbol::new(name);
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(captured, expr)| (captured == &symbol).then(|| expr.clone()))
}

pub(crate) fn expr_label(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(symbol) => symbol.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::List(items) => {
            let labels = items.iter().map(expr_label).collect::<Vec<_>>().join(" ");
            format!("({labels})")
        }
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use sim_kernel::{ClaimPattern, Ref, Symbol, testing::bare_cx as cx};
    use sim_lib_standard_core::{standard_test_capability, standard_test_result_predicate};

    use super::*;
    use crate::prolog_profile_symbol;

    #[test]
    fn prolog_matrix_row_runner_reports_all_current_cases() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let report = run_prolog_matrix_row(&mut cx).unwrap();

        assert_eq!(report.cells.len(), 19);
        assert_eq!(report.pass_count(), 16);
        assert_eq!(report.gap_count(), 3);
        assert_eq!(report.fail_count(), 0);
        assert_eq!(report.language_fidelity(&Symbol::new("prolog")), Some(1.0));
        let claims = cx.query_facts(prolog_profile_result_claims()).unwrap();
        assert_eq!(claims.len(), 19);
    }

    fn prolog_profile_result_claims() -> ClaimPattern {
        ClaimPattern {
            subject: Some(Ref::Symbol(prolog_profile_symbol())),
            predicate: Some(standard_test_result_predicate()),
            object: None,
            include_revoked: false,
        }
    }
}
