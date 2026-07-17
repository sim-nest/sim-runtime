//! Browseable logic builtin bindings.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex},
};

use indexmap::IndexMap;
use sim_kernel::{Cx, Error, Expr, Result, ShapeMatch, Symbol};

use crate::{
    LogicConfig, LogicDb,
    all_solutions::{
        FindallRequest, bagof_through_sequence, findall_through_sequence, setof_through_sequence,
    },
    arith::{eval_compare_through_tower, eval_is_through_tower},
    clause::{predicate_symbol, rename_clause_apart},
    env::LogicEnv,
    error::logic_eval_error,
    lists::{
        append_through_sequence, length_through_sequence, member_through_sequence,
        select_through_sequence,
    },
    query::query_all,
    unify::occurs_check,
};

/// Context handed to every builtin projection.
pub struct BuiltinCtx<'a> {
    /// Active clause database for child queries.
    pub db: &'a LogicDb,
    /// Active query limits and search configuration.
    pub config: &'a LogicConfig,
    /// Effective answer cap for the current query stream.
    pub answer_limit: Option<usize>,
}

/// Projection function used by a builtin binding.
pub type BuiltinSolve = dyn for<'a> Fn(&mut Cx, &BuiltinCtx<'a>, &[Expr], &LogicEnv) -> Result<Vec<LogicEnv>>
    + Send
    + Sync;

type BuiltinProjection =
    for<'a> fn(&mut Cx, &BuiltinCtx<'a>, &[Expr], &LogicEnv) -> Result<Vec<LogicEnv>>;

/// Data record describing one builtin goal.
#[derive(Clone)]
pub struct BuiltinBinding {
    /// Goal functor handled by this binding.
    pub key: Symbol,
    /// Organ that resolves the builtin, exposed as browseable metadata.
    pub organ: Symbol,
    /// Thin projection from a goal's arguments to continuation environments.
    pub solve: Arc<BuiltinSolve>,
}

impl fmt::Debug for BuiltinBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuiltinBinding")
            .field("key", &self.key)
            .field("organ", &self.organ)
            .finish_non_exhaustive()
    }
}

/// Table of builtin goal bindings.
#[derive(Clone, Default)]
pub struct BuiltinTable {
    bindings: IndexMap<Symbol, BuiltinBinding>,
}

impl fmt::Debug for BuiltinTable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuiltinTable")
            .field("keys", &self.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl BuiltinTable {
    /// Registers or replaces a builtin binding.
    pub fn register(&mut self, binding: BuiltinBinding) {
        self.bindings.insert(binding.key.clone(), binding);
    }

    /// Returns the binding for `key`, when one is registered.
    pub fn get(&self, key: &Symbol) -> Option<&BuiltinBinding> {
        self.bindings.get(key)
    }

    /// Returns the organ metadata for `key`, when one is registered.
    pub fn organ_of(&self, key: &Symbol) -> Option<&Symbol> {
        self.bindings.get(key).map(|binding| &binding.organ)
    }

    /// Returns the registered builtin keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &Symbol> {
        self.bindings.keys()
    }

    /// Returns the standard builtin table.
    pub fn standard() -> Self {
        let mut table = Self::default();
        register_keystones(&mut table);
        register_constraints(&mut table);
        register_arithmetic_comparisons(&mut table);
        register_lists(&mut table);
        table
    }
}

/// Builds a sequence-organ tabling memo binding for `predicate`.
///
/// The binding is ordinary [`BuiltinTable`] data: registering it under a
/// predicate key makes that predicate tabled without changing the resolver. It
/// computes a finite fixed point for the predicate's current clauses, caches the
/// ground answer tuples by arity, and replays matching tuples into the active
/// environment on every call.
pub fn tabling_memo_binding(predicate: Symbol) -> BuiltinBinding {
    let memo = Arc::new(Mutex::new(TabledMemo::default()));
    BuiltinBinding {
        key: predicate.clone(),
        organ: Symbol::new("sequence"),
        solve: Arc::new(move |cx, ctx, args, env| {
            let tuples = cached_tabled_tuples(cx, ctx, &predicate, args.len(), &memo)?;
            replay_tabled_tuples(cx, ctx.config, args, env, &tuples)
        }),
    }
}

fn register_keystones(table: &mut BuiltinTable) {
    table.register(BuiltinBinding {
        key: Symbol::new("is"),
        organ: Symbol::qualified("numbers", "arith"),
        solve: Arc::new(|cx, ctx, args, env| {
            let [left, right] = args else {
                return Err(logic_eval_error("is expects two arguments"));
            };
            eval_is_through_tower(cx, ctx.config, left, right, env)
        }),
    });
    table.register(BuiltinBinding {
        key: Symbol::new("findall"),
        organ: Symbol::new("sequence"),
        solve: Arc::new(|cx, ctx, args, env| {
            let [template, goal, output] = args else {
                return Err(logic_eval_error("findall expects three arguments"));
            };
            findall_through_sequence(
                cx,
                FindallRequest {
                    db: ctx.db,
                    config: ctx.config,
                    template,
                    goal,
                    output,
                    env,
                },
            )
        }),
    });
    register_sequence_binding(table, "bagof", bagof_through_sequence);
    register_sequence_binding(table, "setof", setof_through_sequence);
}

fn register_constraints(table: &mut BuiltinTable) {
    for key in ["#=", "#<", "dif"] {
        let key = Symbol::new(key);
        table.register(BuiltinBinding {
            key: key.clone(),
            organ: Symbol::new("control"),
            solve: Arc::new(move |cx, ctx, args, env| {
                crate::constraints::solve_constraint(cx, ctx.config, &key, args, env)
            }),
        });
    }
    for key in [
        "=",
        "<",
        "<=",
        ">",
        ">=",
        "plus",
        "minus",
        "times",
        "between",
        "tool-call",
    ] {
        let key = Symbol::new(key);
        table.register(BuiltinBinding {
            key: key.clone(),
            organ: Symbol::qualified("logic", "constraint"),
            solve: Arc::new(move |cx, ctx, args, env| {
                crate::constraints::solve_constraint(cx, ctx.config, &key, args, env)
            }),
        });
    }
}

fn register_arithmetic_comparisons(table: &mut BuiltinTable) {
    for key in ["=:=", "=\\=", "<", "=<", ">", ">="] {
        let key = Symbol::new(key);
        table.register(BuiltinBinding {
            key: key.clone(),
            organ: Symbol::qualified("numbers", "arith"),
            solve: Arc::new(move |cx, _ctx, args, env| {
                eval_compare_through_tower(cx, &key, args, env)
            }),
        });
    }
}

fn register_lists(table: &mut BuiltinTable) {
    register_sequence_binding(table, "member", member_through_sequence);
    register_sequence_binding(table, "append", append_through_sequence);
    register_sequence_binding(table, "length", length_through_sequence);
    register_sequence_binding(table, "select", select_through_sequence);
}

fn register_sequence_binding(table: &mut BuiltinTable, key: &str, solve: BuiltinProjection) {
    table.register(BuiltinBinding {
        key: Symbol::new(key),
        organ: Symbol::new("sequence"),
        solve: Arc::new(solve),
    });
}

#[derive(Default)]
struct TabledMemo {
    by_arity: BTreeMap<usize, Vec<Vec<Expr>>>,
}

fn cached_tabled_tuples(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    predicate: &Symbol,
    arity: usize,
    memo: &Arc<Mutex<TabledMemo>>,
) -> Result<Vec<Vec<Expr>>> {
    if let Some(cached) = memo
        .lock()
        .map_err(|_| Error::PoisonedLock("logic tabling memo"))?
        .by_arity
        .get(&arity)
        .cloned()
    {
        return Ok(cached);
    }

    let computed = compute_tabled_tuples(cx, ctx, predicate, arity)?;
    let mut guard = memo
        .lock()
        .map_err(|_| Error::PoisonedLock("logic tabling memo"))?;
    Ok(guard
        .by_arity
        .entry(arity)
        .or_insert_with(|| computed.clone())
        .clone())
}

fn compute_tabled_tuples(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    predicate: &Symbol,
    arity: usize,
) -> Result<Vec<Vec<Expr>>> {
    let mut tuples = Vec::new();
    let mut seen = BTreeSet::new();
    let max_rounds = ctx.config.limits.max_depth.max(1);
    for round in 0..max_rounds {
        let before = tuples.len();
        for clause in ctx.db.clauses() {
            if clause.predicate()? != predicate.clone() || clause.arity()? != arity {
                continue;
            }
            let clause = rename_clause_apart(clause, round + 1);
            for env in solve_tabled_body(cx, ctx, predicate, &tuples, &clause.body)? {
                let tuple = tabled_head_tuple(&clause.head, &env)?;
                if tuple.len() == arity
                    && tuple.iter().all(is_ground)
                    && seen.insert(tuple_key(&tuple))
                {
                    tuples.push(tuple);
                }
            }
        }
        if tuples.len() == before {
            return Ok(tuples);
        }
    }
    Err(logic_eval_error(format!(
        "tabling memo for {predicate} exceeded fixed-point limit {max_rounds}"
    )))
}

fn solve_tabled_body(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    predicate: &Symbol,
    tuples: &[Vec<Expr>],
    body: &[Expr],
) -> Result<Vec<LogicEnv>> {
    let mut envs = vec![LogicEnv::new()];
    for goal in body {
        let mut next_envs = Vec::new();
        for env in envs {
            let applied = env.apply(goal);
            if predicate_symbol(&applied)? == predicate.clone() {
                next_envs.extend(replay_tabled_tuples(
                    cx,
                    ctx.config,
                    goal_args(&applied)?,
                    &env,
                    tuples,
                )?);
            } else {
                next_envs.extend(resolve_non_tabled_goal(cx, ctx, &applied, &env)?);
            }
        }
        envs = next_envs;
        if envs.is_empty() {
            break;
        }
    }
    Ok(envs)
}

fn replay_tabled_tuples(
    cx: &mut Cx,
    config: &LogicConfig,
    args: &[Expr],
    env: &LogicEnv,
    tuples: &[Vec<Expr>],
) -> Result<Vec<LogicEnv>> {
    let mut out = Vec::new();
    for tuple in tuples.iter().filter(|tuple| tuple.len() == args.len()) {
        let mut next = env.clone();
        let mut accepted = true;
        for (arg, value) in args.iter().zip(tuple) {
            if !next.unify(cx, arg, value, occurs_check(config))? {
                accepted = false;
                break;
            }
        }
        if accepted {
            out.push(next);
        }
    }
    Ok(out)
}

fn resolve_non_tabled_goal(
    cx: &mut Cx,
    ctx: &BuiltinCtx<'_>,
    goal: &Expr,
    env: &LogicEnv,
) -> Result<Vec<LogicEnv>> {
    let mut out = Vec::new();
    for answer in query_all(cx, ctx.db, ctx.config, goal.clone(), ctx.answer_limit)? {
        if let Some(next) = merge_answer(cx, env.clone(), ctx.config, &answer)? {
            out.push(next);
        }
    }
    Ok(out)
}

fn merge_answer(
    cx: &mut Cx,
    mut env: LogicEnv,
    config: &LogicConfig,
    answer: &ShapeMatch,
) -> Result<Option<LogicEnv>> {
    for (var, value) in answer.captures.exprs() {
        if !env.unify(cx, &Expr::Local(var.clone()), value, occurs_check(config))? {
            return Ok(None);
        }
    }
    Ok(Some(env))
}

fn tabled_head_tuple(head: &Expr, env: &LogicEnv) -> Result<Vec<Expr>> {
    Ok(goal_args(head)?.iter().map(|arg| env.apply(arg)).collect())
}

fn goal_args(goal: &Expr) -> Result<&[Expr]> {
    match goal {
        Expr::List(items) => Ok(&items[1..]),
        Expr::Call { args, .. } => Ok(args),
        _ => Err(logic_eval_error("tabled goal must be call-shaped")),
    }
}

fn is_ground(expr: &Expr) -> bool {
    match expr {
        Expr::Local(_) => false,
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().all(is_ground)
        }
        Expr::Map(entries) => entries
            .iter()
            .all(|(key, value)| is_ground(key) && is_ground(value)),
        Expr::Call { operator, args } => is_ground(operator) && args.iter().all(is_ground),
        Expr::Infix { left, right, .. } => is_ground(left) && is_ground(right),
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => is_ground(arg),
        Expr::Quote { expr, .. } | Expr::Extension { payload: expr, .. } => is_ground(expr),
        Expr::Annotated { expr, annotations } => {
            is_ground(expr) && annotations.iter().all(|(_, value)| is_ground(value))
        }
        _ => true,
    }
}

fn tuple_key(tuple: &[Expr]) -> String {
    tuple
        .iter()
        .map(|expr| format!("{:?}", expr.canonical_key()))
        .collect::<Vec<_>>()
        .join("\0")
}
