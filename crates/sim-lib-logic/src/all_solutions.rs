use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{CanonicalKey, Cx, Expr, Result, ShapeMatch, Symbol};

use crate::{
    builtins::BuiltinCtx, db::LogicDb, env::LogicEnv, error::logic_eval_error, model::LogicConfig,
    query::SequenceEngine, unify::occurs_check,
};

pub(crate) struct FindallRequest<'a> {
    pub(crate) db: &'a LogicDb,
    pub(crate) config: &'a LogicConfig,
    pub(crate) template: &'a Expr,
    pub(crate) goal: &'a Expr,
    pub(crate) output: &'a Expr,
    pub(crate) env: &'a LogicEnv,
}

pub(crate) fn findall_through_sequence(
    cx: &mut Cx,
    request: FindallRequest<'_>,
) -> Result<Vec<LogicEnv>> {
    findall_through_sequence_with_probe(cx, request, |_| {})
}

pub(crate) fn bagof_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    grouped_all_solutions(cx, ctx, args, env, false)
}

pub(crate) fn setof_through_sequence(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    grouped_all_solutions(cx, ctx, args, env, true)
}

pub(crate) fn findall_through_sequence_with_probe(
    cx: &mut Cx,
    request: FindallRequest<'_>,
    mut on_forced_answer: impl FnMut(&ShapeMatch),
) -> Result<Vec<LogicEnv>> {
    let projected_goal = request.env.apply(request.goal);
    let projected_template = request.env.apply(request.template);
    let answer_limit = request.config.limits.max_answers;
    let engine = SequenceEngine::new(
        request.db.clone(),
        request.config.clone(),
        projected_goal,
        answer_limit,
    )?;
    let mut values = Vec::new();
    while let Some(answer) = engine.next_match(cx)? {
        on_forced_answer(&answer);
        values.push(project_template("findall", &projected_template, &answer)?);
    }
    engine.close(cx)?;

    let mut next = request.env.clone();
    if next.unify(
        cx,
        request.output,
        &Expr::List(values),
        occurs_check(request.config),
    )? {
        Ok(vec![next])
    } else {
        Ok(Vec::new())
    }
}

fn grouped_all_solutions(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    args: &[Expr],
    env: &LogicEnv,
    dedup: bool,
) -> Result<Vec<LogicEnv>> {
    let [template, qualified_goal, output] = args else {
        return Err(logic_eval_error("bagof/setof expect three arguments"));
    };
    let (existential, goal) = strip_existential(qualified_goal);
    let projected_goal = env.apply(&goal);
    let projected_template = env.apply(template);
    let template_vars = env.free_vars(&projected_template).into_iter().collect();
    let witness_vars = witness_vars(env, &projected_goal, &template_vars, &existential);
    let groups = collect_groups(cx, ctx, &projected_template, projected_goal, &witness_vars)?;
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    bind_groups(cx, ctx, env, output, groups, dedup)
}

fn collect_groups(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    projected_template: &Expr,
    projected_goal: Expr,
    witness_vars: &[Symbol],
) -> Result<BTreeMap<Vec<CanonicalKey>, AnswerGroup>> {
    let engine = SequenceEngine::new(
        ctx.db.clone(),
        ctx.config.clone(),
        projected_goal,
        ctx.config.limits.max_answers,
    )?;
    let mut groups: BTreeMap<Vec<CanonicalKey>, AnswerGroup> = BTreeMap::new();
    while let Some(answer) = engine.next_match(cx)? {
        let witnesses = witness_bindings("bagof/setof", witness_vars, &answer)?;
        let key = witnesses
            .iter()
            .map(|(_symbol, expr)| expr.canonical_key())
            .collect::<Vec<_>>();
        let group = groups.entry(key).or_insert_with(|| AnswerGroup {
            witnesses,
            values: Vec::new(),
        });
        group.values.push(project_template(
            "bagof/setof",
            projected_template,
            &answer,
        )?);
    }
    engine.close(cx)?;
    Ok(groups)
}

fn bind_groups(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    env: &LogicEnv,
    output: &Expr,
    groups: BTreeMap<Vec<CanonicalKey>, AnswerGroup>,
    dedup: bool,
) -> Result<Vec<LogicEnv>> {
    let mut answers = Vec::new();
    for (_key, mut group) in groups {
        if dedup {
            sort_dedup_terms(&mut group.values);
        }
        let mut next = env.clone();
        let mut witnesses_match = true;
        for (symbol, value) in group.witnesses {
            if !next.unify(cx, &Expr::Local(symbol), &value, occurs_check(ctx.config))? {
                witnesses_match = false;
                break;
            }
        }
        if !witnesses_match {
            continue;
        }
        if next.unify(
            cx,
            output,
            &Expr::List(group.values),
            occurs_check(ctx.config),
        )? {
            answers.push(next);
        }
    }
    Ok(answers)
}

fn strip_existential(expr: &Expr) -> (BTreeSet<Symbol>, Expr) {
    let Expr::List(items) = expr else {
        return (BTreeSet::new(), expr.clone());
    };
    let [head, qualified, goal] = items.as_slice() else {
        return (BTreeSet::new(), expr.clone());
    };
    let Expr::Symbol(symbol) = head else {
        return (BTreeSet::new(), expr.clone());
    };
    if symbol.name.as_ref() != "^" || symbol.namespace.is_some() {
        return (BTreeSet::new(), expr.clone());
    }
    let (mut vars, goal) = strip_existential(goal);
    collect_local_vars(qualified, &mut vars);
    (vars, goal)
}

fn witness_vars(
    env: &LogicEnv,
    projected_goal: &Expr,
    template_vars: &BTreeSet<Symbol>,
    existential: &BTreeSet<Symbol>,
) -> Vec<Symbol> {
    env.free_vars(projected_goal)
        .into_iter()
        .filter(|symbol| !template_vars.contains(symbol) && !existential.contains(symbol))
        .collect()
}

fn collect_local_vars(expr: &Expr, vars: &mut BTreeSet<Symbol>) {
    match expr {
        Expr::Local(symbol) => {
            vars.insert(symbol.clone());
        }
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            for item in items {
                collect_local_vars(item, vars);
            }
        }
        Expr::Map(entries) => {
            for (key, value) in entries {
                collect_local_vars(key, vars);
                collect_local_vars(value, vars);
            }
        }
        Expr::Call { operator, args } => {
            collect_local_vars(operator, vars);
            for arg in args {
                collect_local_vars(arg, vars);
            }
        }
        Expr::Infix { left, right, .. } => {
            collect_local_vars(left, vars);
            collect_local_vars(right, vars);
        }
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => collect_local_vars(arg, vars),
        Expr::Quote { expr, .. } => collect_local_vars(expr, vars),
        Expr::Annotated { expr, annotations } => {
            collect_local_vars(expr, vars);
            for (_symbol, value) in annotations {
                collect_local_vars(value, vars);
            }
        }
        Expr::Extension { payload, .. } => collect_local_vars(payload, vars),
        _ => {}
    }
}

fn witness_bindings(
    context: &str,
    witness_vars: &[Symbol],
    answer: &ShapeMatch,
) -> Result<Vec<(Symbol, Expr)>> {
    witness_vars
        .iter()
        .map(|symbol| Ok((symbol.clone(), capture_expr(context, answer, symbol)?)))
        .collect()
}

fn sort_dedup_terms(values: &mut Vec<Expr>) {
    values.sort_by_key(Expr::canonical_key);
    values.dedup_by(|left, right| left.canonical_eq(right));
}

struct AnswerGroup {
    witnesses: Vec<(Symbol, Expr)>,
    values: Vec<Expr>,
}

fn project_template(context: &str, template: &Expr, answer: &ShapeMatch) -> Result<Expr> {
    match template {
        Expr::Local(symbol) => capture_expr(context, answer, symbol),
        Expr::List(items) => items
            .iter()
            .map(|item| project_template(context, item, answer))
            .collect::<Result<Vec<_>>>()
            .map(Expr::List),
        Expr::Vector(items) => items
            .iter()
            .map(|item| project_template(context, item, answer))
            .collect::<Result<Vec<_>>>()
            .map(Expr::Vector),
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| {
                Ok((
                    project_template(context, key, answer)?,
                    project_template(context, value, answer)?,
                ))
            })
            .collect::<Result<Vec<_>>>()
            .map(Expr::Map),
        Expr::Set(items) => items
            .iter()
            .map(|item| project_template(context, item, answer))
            .collect::<Result<Vec<_>>>()
            .map(Expr::Set),
        other => Ok(other.clone()),
    }
}

fn capture_expr(context: &str, answer: &ShapeMatch, symbol: &Symbol) -> Result<Expr> {
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(name, expr)| (name == symbol).then(|| expr.clone()))
        .ok_or_else(|| logic_eval_error(format!("{context} variable ?{} is unbound", symbol.name,)))
}
