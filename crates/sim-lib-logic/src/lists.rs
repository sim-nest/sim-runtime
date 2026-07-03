//! List builtin projections through the sequence organ.

use sim_kernel::{Cx, Expr, NumberLiteral, Result, Symbol, Value};
use sim_lib_sequence::{force_sequence_bounded, persistent_list, sequence_from_list_value};

use crate::{builtins::BuiltinCtx, env::LogicEnv, error::logic_eval_error, unify::occurs_check};

pub(crate) fn member_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [needle, list] = args else {
        return Err(logic_eval_error("member expects two arguments"));
    };
    let items = list_items_through_sequence(cx, ctx, &env.apply(list), "member")?;
    let mut answers = Vec::new();
    for item in items {
        let mut next = env.clone();
        if next.unify(needle, &item, occurs_check(ctx.config))? {
            answers.push(next);
        }
    }
    Ok(answers)
}

pub(crate) fn append_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [left, right, output] = args else {
        return Err(logic_eval_error("append expects three arguments"));
    };

    let applied_output = env.apply(output);
    if matches!(applied_output, Expr::List(_)) {
        return append_splits_through_sequence(cx, ctx, left, right, &applied_output, env);
    }

    let left_items = list_items_through_sequence(cx, ctx, &env.apply(left), "append left")?;
    let right_items = list_items_through_sequence(cx, ctx, &env.apply(right), "append right")?;
    let mut concatenated = left_items;
    concatenated.extend(right_items);

    let mut next = env.clone();
    if next.unify(output, &Expr::List(concatenated), occurs_check(ctx.config))? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

pub(crate) fn length_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [list, output] = args else {
        return Err(logic_eval_error("length expects two arguments"));
    };
    let items = list_items_through_sequence(cx, ctx, &env.apply(list), "length")?;
    let mut next = env.clone();
    let length = Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: items.len().to_string(),
    });
    if next.unify(output, &length, occurs_check(ctx.config))? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

pub(crate) fn select_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let [selected, list, remainder] = args else {
        return Err(logic_eval_error("select expects three arguments"));
    };
    let items = list_items_through_sequence(cx, ctx, &env.apply(list), "select")?;
    let mut answers = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let remainder_items = items
            .iter()
            .enumerate()
            .filter(|(candidate, _expr)| *candidate != index)
            .map(|(_candidate, expr)| expr.clone())
            .collect::<Vec<_>>();
        let mut next = env.clone();
        if next.unify(selected, item, occurs_check(ctx.config))?
            && next.unify(
                remainder,
                &Expr::List(remainder_items),
                occurs_check(ctx.config),
            )?
        {
            answers.push(next);
        }
    }
    Ok(answers)
}

fn append_splits_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    left: &Expr,
    right: &Expr,
    output: &Expr,
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let items = list_items_through_sequence(cx, ctx, output, "append output")?;
    let mut answers = Vec::new();
    for split_at in 0..=items.len() {
        let mut next = env.clone();
        let prefix = Expr::List(items[..split_at].to_vec());
        let suffix = Expr::List(items[split_at..].to_vec());
        if next.unify(left, &prefix, occurs_check(ctx.config))?
            && next.unify(right, &suffix, occurs_check(ctx.config))?
        {
            answers.push(next);
            if answers.len() >= sequence_answer_bound(ctx) {
                break;
            }
        }
    }
    Ok(answers)
}

fn list_items_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    expr: &Expr,
    context: &str,
) -> Result<Vec<Expr>> {
    let Expr::List(items) = expr else {
        return Err(logic_eval_error(format!(
            "{context}: expected closed Prolog list expression"
        )));
    };
    let sequence = sequence_value_from_exprs(cx, items)?;
    force_sequence_bounded(cx, &sequence, sequence_answer_bound(ctx), context)?
        .into_iter()
        .map(|value| value.object().as_expr(cx))
        .collect()
}

fn sequence_value_from_exprs(cx: &mut Cx, items: &[Expr]) -> Result<Value> {
    let values = items
        .iter()
        .map(|item| cx.factory().expr(item.clone()))
        .collect::<Result<Vec<_>>>()?;
    let list = persistent_list(cx, values)?;
    sequence_from_list_value(cx, list)
}

fn sequence_answer_bound(ctx: &BuiltinCtx<'_>) -> usize {
    let config_bound = ctx
        .config
        .limits
        .max_answers
        .unwrap_or(ctx.config.limits.max_clause_scan);
    ctx.answer_limit
        .map(|answer_limit| answer_limit.min(config_bound))
        .unwrap_or(config_bound)
}
