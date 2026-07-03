use sim_kernel::Result;

use crate::conformance::{
    assert_clause, binding_expr, expr_label, fact, goal, list, local, prolog_case_cx, query_all,
    symbol,
};

pub(crate) fn run_findall_duplicates_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "findall",
            vec![
                local("x"),
                goal(
                    "member",
                    vec![
                        local("x"),
                        list(vec![symbol("a"), symbol("b"), symbol("a")]),
                    ],
                ),
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
        "prolog:findall organ=sequence answers={} xs={xs}",
        answers.len()
    ))
}

pub(crate) fn run_bagof_groups_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    for (parent, child) in [("alice", "bob"), ("alice", "bea"), ("cara", "drew")] {
        assert_clause(&mut cx, fact("parent", vec![symbol(parent), symbol(child)]))?;
    }
    let answers = query_all(
        &mut cx,
        goal(
            "bagof",
            vec![
                local("child"),
                goal("parent", vec![local("parent"), local("child")]),
                local("children"),
            ],
        ),
        4,
    )?;
    let groups = answers
        .iter()
        .map(|answer| {
            let parent = binding_expr(answer, "parent")
                .map(|expr| expr_label(&expr))
                .unwrap_or_else(|| "none".to_owned());
            let children = binding_expr(answer, "children")
                .map(|expr| expr_label(&expr))
                .unwrap_or_else(|| "none".to_owned());
            format!("{parent}:{children}")
        })
        .collect::<Vec<_>>()
        .join(";");
    Ok(format!(
        "prolog:bagof organ=sequence answers={} groups={groups}",
        answers.len()
    ))
}

pub(crate) fn run_setof_sorted_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "setof",
            vec![
                local("x"),
                goal(
                    "member",
                    vec![
                        local("x"),
                        list(vec![symbol("c"), symbol("a"), symbol("b"), symbol("a")]),
                    ],
                ),
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
        "prolog:setof organ=sequence answers={} xs={xs}",
        answers.len()
    ))
}

pub(crate) fn run_bagof_empty_case() -> Result<String> {
    let mut cx = prolog_case_cx()?;
    let answers = query_all(
        &mut cx,
        goal(
            "bagof",
            vec![
                local("x"),
                goal("member", vec![local("x"), list(Vec::new())]),
                local("xs"),
            ],
        ),
        4,
    )?;
    Ok(format!(
        "prolog:bagof-empty organ=sequence answers={}",
        answers.len()
    ))
}
