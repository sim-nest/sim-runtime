use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{Arc, Mutex},
};

use sim_kernel::{Cx, Error, Expr, Result, Sequence, ShapeMatch, Symbol, Value, shape_match_value};
use sim_lib_sequence::{LazySequence, SequenceProducer};

use crate::{
    builtins::{BuiltinCtx, BuiltinTable},
    clause::{
        is_cut_expr, is_goal_expr, normalize_goal_expr, predicate_symbol, rename_clause_apart,
    },
    cut::{CutPrompt, raise_cut_prompt},
    db::LogicDb,
    env::LogicEnv,
    error::logic_eval_error,
    model::{LogicConfig, SearchStrategy},
    naf::{NafDemand, naf_inner_goal},
    stream::LogicStream,
    unify::occurs_check,
};

/// A goal paired with the [`LogicConfig`] under which it should be resolved.
#[derive(Clone, Debug)]
pub struct LogicQuery {
    /// The goal expression to prove.
    pub goal: Expr,
    /// Tuning applied while resolving the goal.
    pub config: LogicConfig,
}

/// Resolves `goal` against `db` and returns the answers as a logic stream.
///
/// Resolution is bounded by `config`; the answer count is capped at
/// `config.limits.max_answers`.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};
/// use sim_lib_logic::{LogicConfig, LogicDb, query};
///
/// let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
/// let mut db = LogicDb::new();
/// db.assert_clause_expr(Expr::List(vec![
///     Expr::Symbol(Symbol::new("fact")),
///     Expr::List(vec![
///         Expr::Symbol(Symbol::new("parent")),
///         Expr::Symbol(Symbol::new("alice")),
///         Expr::Symbol(Symbol::new("bob")),
///     ]),
/// ]))
/// .unwrap();
///
/// let goal = Expr::List(vec![
///     Expr::Symbol(Symbol::new("parent")),
///     Expr::Symbol(Symbol::new("alice")),
///     Expr::Local(Symbol::new("who")),
/// ]);
/// let stream = query(&mut cx, &db, &LogicConfig::default(), goal).unwrap();
/// let answers = stream.collect(&mut cx, None).unwrap();
/// assert_eq!(answers.len(), 1);
/// ```
pub fn query(cx: &mut Cx, db: &LogicDb, config: &LogicConfig, goal: Expr) -> Result<LogicStream> {
    let _ = cx;
    let engine = SequenceEngine::new(db.clone(), config.clone(), goal, config.limits.max_answers)?;
    Ok(LogicStream::from_engine(engine, config.stream_buffer))
}

/// Resolves `goal` against `db` and returns all answers up to `limit`.
///
/// When `limit` is `None`, the answer count is capped by
/// `config.limits.max_answers`. Each answer is a [`ShapeMatch`] whose captures
/// contain the variables bound while proving the goal.
pub fn query_all(
    cx: &mut Cx,
    db: &LogicDb,
    config: &LogicConfig,
    goal: Expr,
    limit: Option<usize>,
) -> Result<Vec<ShapeMatch>> {
    let answer_limit = limit.or(config.limits.max_answers);
    let mut engine = SequenceEngine::new(db.clone(), config.clone(), goal, answer_limit)?;
    engine.force(cx, answer_limit)
}

/// Resolves `goal` with an explicit builtin table.
///
/// This is the open-registry query surface for callers that install additional
/// builtin bindings as data. It uses the same resolver as [`query_all`], but
/// starts the stream with `builtins` instead of [`BuiltinTable::standard`].
pub fn query_all_with_builtins(
    cx: &mut Cx,
    db: &LogicDb,
    config: &LogicConfig,
    goal: Expr,
    limit: Option<usize>,
    builtins: BuiltinTable,
) -> Result<Vec<ShapeMatch>> {
    let answer_limit = limit.or(config.limits.max_answers);
    let mut engine = SequenceEngine::new_with_builtins(
        db.clone(),
        config.clone(),
        goal,
        answer_limit,
        builtins,
    )?;
    engine.force(cx, answer_limit)
}

pub(crate) struct SequenceEngine {
    sequence: LazySequence,
    state: Arc<Mutex<SequenceEngineState>>,
}

impl SequenceEngine {
    pub(crate) fn new(
        db: LogicDb,
        config: LogicConfig,
        goal: Expr,
        answer_limit: Option<usize>,
    ) -> Result<Self> {
        Self::new_with_builtins(db, config, goal, answer_limit, BuiltinTable::standard())
    }

    pub(crate) fn new_with_builtins(
        db: LogicDb,
        config: LogicConfig,
        goal: Expr,
        answer_limit: Option<usize>,
        builtins: BuiltinTable,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(SequenceEngineState::new(
            db,
            config,
            goal,
            answer_limit,
            builtins,
        )?));
        let producer = sequence_producer(Arc::clone(&state));
        Ok(Self {
            sequence: LazySequence::new(producer),
            state,
        })
    }

    pub(crate) fn next_match(&self, cx: &mut Cx) -> Result<Option<ShapeMatch>> {
        let Some(item) = self.sequence.next_item(cx)? else {
            return Ok(None);
        };
        let _ = item.into_value(cx)?;
        self.pop_emitted()
    }

    pub(crate) fn next_value(&self, cx: &mut Cx) -> Result<Option<Value>> {
        let Some(item) = self.sequence.next_item(cx)? else {
            return Ok(None);
        };
        let value = item.into_value(cx)?;
        let _ = self.pop_emitted()?;
        Ok(Some(value))
    }

    pub(crate) fn close(&self, cx: &mut Cx) -> Result<()> {
        self.sequence.close(cx)
    }

    fn force(&mut self, cx: &mut Cx, limit: Option<usize>) -> Result<Vec<ShapeMatch>> {
        let bound = limit.unwrap_or(usize::MAX);
        let mut answers = Vec::new();
        while answers.len() < bound {
            let Some(answer) = self.next_match(cx)? else {
                break;
            };
            answers.push(answer);
        }
        Ok(answers)
    }

    fn pop_emitted(&self) -> Result<Option<ShapeMatch>> {
        self.state
            .lock()
            .map_err(|_| Error::PoisonedLock("logic sequence"))?
            .emitted
            .pop_front()
            .map(Some)
            .ok_or_else(|| logic_eval_error("logic sequence emitted no answer"))
    }
}

impl Clone for SequenceEngine {
    fn clone(&self) -> Self {
        Self {
            sequence: self.sequence.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

impl fmt::Debug for SequenceEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SequenceEngine")
    }
}

#[derive(Debug)]
struct SequenceEngineState {
    db: LogicDb,
    config: LogicConfig,
    frames: Vec<ResolveFrame>,
    clause_scans: usize,
    seen: BTreeMap<String, usize>,
    builtins: BuiltinTable,
    emitted: VecDeque<ShapeMatch>,
    answers_emitted: usize,
    answer_limit: Option<usize>,
    next_choice_frame_id: u64,
}

impl SequenceEngineState {
    fn new(
        db: LogicDb,
        config: LogicConfig,
        goal: Expr,
        answer_limit: Option<usize>,
        builtins: BuiltinTable,
    ) -> Result<Self> {
        let goal = normalize_goal_expr(&goal);
        if !is_goal_expr(&goal) {
            return Err(logic_eval_error("query goal must be a call-shaped list"));
        }
        Ok(Self {
            db,
            config,
            frames: vec![ResolveFrame {
                goals: vec![goal],
                env: LogicEnv::new(),
                depth: 0,
                choice_frame_id: 0,
                choice_parent: 0,
            }],
            clause_scans: 0,
            seen: BTreeMap::new(),
            builtins,
            emitted: VecDeque::new(),
            answers_emitted: 0,
            answer_limit,
            next_choice_frame_id: 0,
        })
    }

    fn step(&mut self, cx: &mut Cx) -> Result<Option<ShapeMatch>> {
        if self
            .answer_limit
            .is_some_and(|limit| self.answers_emitted >= limit)
        {
            return Ok(None);
        }
        while let Some(frame) = pop_frame(&mut self.frames, self.config.strategy) {
            if frame.depth > self.config.limits.max_depth {
                return Err(logic_eval_error(format!(
                    "logic query exceeded max_depth {}",
                    self.config.limits.max_depth
                )));
            }
            if frame.goals.len() > self.config.limits.max_goals {
                return Err(logic_eval_error(format!(
                    "logic query exceeded max_goals {}",
                    self.config.limits.max_goals
                )));
            }
            if frame.goals.is_empty() {
                self.answers_emitted += 1;
                return frame.env.as_shape_match(cx).map(Some);
            }

            let goal = frame.env.apply(&frame.goals[0]);
            let rest = frame.goals[1..].to_vec();
            if let Some(inner) = naf_inner_goal(&goal)? {
                let grounded = frame.env.apply(inner);
                if !frame.env.free_vars(&grounded).is_empty() {
                    return Err(logic_eval_error(
                        "negation-as-failure flounders on unbound variables",
                    ));
                }
                let demand = NafDemand::new(grounded);
                let _tag = demand.tag();
                let has_answer = self.naf_has_answer(cx, demand.goal().clone())?;
                if !has_answer {
                    push_frame(
                        &mut self.frames,
                        ResolveFrame {
                            goals: rest,
                            env: frame.env,
                            depth: frame.depth,
                            choice_frame_id: frame.choice_frame_id,
                            choice_parent: frame.choice_parent,
                        },
                        self.config.strategy,
                    );
                }
                continue;
            }
            if is_cut_expr(&goal) {
                raise_cut_prompt(
                    cx,
                    CutPrompt {
                        cut_parent: frame.choice_parent,
                    },
                )?;
                self.frames
                    .retain(|pending| pending.choice_frame_id <= frame.choice_parent);
                push_frame(
                    &mut self.frames,
                    ResolveFrame {
                        goals: rest,
                        env: frame.env,
                        depth: frame.depth,
                        choice_frame_id: frame.choice_frame_id,
                        choice_parent: frame.choice_parent,
                    },
                    self.config.strategy,
                );
                continue;
            }

            let table_key = format!("{:?}", normalize_goal_expr(&goal).canonical_key());
            let visits = self.seen.entry(table_key).or_default();
            *visits += 1;
            if self.config.enable_tabling && *visits > self.config.limits.max_depth {
                return Err(logic_eval_error(format!(
                    "logic query exceeded max_depth {}",
                    self.config.limits.max_depth
                )));
            }

            if let Some(symbol) = builtin_head(&goal)
                && let Some(solve) = self
                    .builtins
                    .get(&symbol)
                    .map(|binding| Arc::clone(&binding.solve))
            {
                let args = goal_args(&goal)?;
                let ctx = BuiltinCtx {
                    db: &self.db,
                    config: &self.config,
                    answer_limit: self.answer_limit,
                };
                let choice_parent = frame.choice_frame_id;
                let frames = solve(cx, &ctx, args, &frame.env)?
                    .into_iter()
                    .map(|next_env| ResolveFrame {
                        goals: rest.clone(),
                        env: next_env,
                        depth: frame.depth + 1,
                        choice_frame_id: self.next_choice_id(),
                        choice_parent,
                    })
                    .collect();
                push_choice_frames(&mut self.frames, frames, self.config.strategy);
                continue;
            }

            let candidates = self
                .db
                .clauses_for_goal(&goal, self.config.enable_indexing)?
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            let choice_parent = frame.choice_frame_id;
            let mut next_frames = Vec::new();
            for clause in &candidates {
                self.clause_scans += 1;
                if self.clause_scans > self.config.limits.max_clause_scan {
                    return Err(logic_eval_error(format!(
                        "logic query exceeded max_clause_scan {}",
                        self.config.limits.max_clause_scan
                    )));
                }
                let clause = rename_clause_apart(clause, frame.depth + 1);
                let mut next_env = frame.env.clone();
                if !next_env.unify(cx, &goal, &clause.head, occurs_check(&self.config))? {
                    continue;
                }
                let mut goals = clause.body.clone();
                goals.extend(rest.clone());
                next_frames.push(ResolveFrame {
                    goals,
                    env: next_env,
                    depth: frame.depth + 1,
                    choice_frame_id: self.next_choice_id(),
                    choice_parent,
                });
            }
            push_choice_frames(&mut self.frames, next_frames, self.config.strategy);
        }

        Ok(None)
    }

    fn naf_has_answer(&self, cx: &mut Cx, goal: Expr) -> Result<bool> {
        let mut child_config = self.config.clone();
        child_config.limits.max_answers = Some(1);
        let child = SequenceEngine::new(self.db.clone(), child_config, goal, Some(1))?;
        child.next_match(cx).map(|answer| answer.is_some())
    }

    fn next_choice_id(&mut self) -> u64 {
        self.next_choice_frame_id += 1;
        self.next_choice_frame_id
    }
}

fn sequence_producer(state: Arc<Mutex<SequenceEngineState>>) -> SequenceProducer {
    Arc::new(move |cx, _index| {
        let answer = {
            state
                .lock()
                .map_err(|_| Error::PoisonedLock("logic sequence"))?
                .step(cx)?
        };
        let Some(answer) = answer else {
            return Ok(None);
        };
        let value = shape_match_value(cx, answer.clone())?;
        state
            .lock()
            .map_err(|_| Error::PoisonedLock("logic sequence"))?
            .emitted
            .push_back(answer);
        Ok(Some(value))
    })
}

pub fn query_one(
    cx: &mut Cx,
    db: &LogicDb,
    config: &LogicConfig,
    goal: Expr,
) -> Result<Option<ShapeMatch>> {
    Ok(query_all(cx, db, config, goal, Some(1))?.into_iter().next())
}

pub fn query_bool(cx: &mut Cx, db: &LogicDb, config: &LogicConfig, goal: Expr) -> Result<bool> {
    Ok(query_one(cx, db, config, goal)?.is_some())
}

#[derive(Clone, Debug)]
struct ResolveFrame {
    goals: Vec<Expr>,
    env: LogicEnv,
    depth: usize,
    choice_frame_id: u64,
    choice_parent: u64,
}

fn builtin_head(goal: &Expr) -> Option<Symbol> {
    predicate_symbol(goal).ok()
}

fn goal_args(goal: &Expr) -> Result<&[Expr]> {
    match goal {
        Expr::List(items) => Ok(&items[1..]),
        Expr::Call { args, .. } => Ok(args),
        _ => Err(logic_eval_error("builtin goal must be call-shaped")),
    }
}

fn pop_frame(frames: &mut Vec<ResolveFrame>, strategy: SearchStrategy) -> Option<ResolveFrame> {
    match strategy {
        SearchStrategy::Dfs => frames.pop(),
        SearchStrategy::Bfs | SearchStrategy::Fair => {
            (!frames.is_empty()).then(|| frames.remove(0))
        }
    }
}

fn push_frame(frames: &mut Vec<ResolveFrame>, frame: ResolveFrame, strategy: SearchStrategy) {
    match strategy {
        SearchStrategy::Dfs | SearchStrategy::Bfs | SearchStrategy::Fair => frames.push(frame),
    }
}

fn push_choice_frames(
    frames: &mut Vec<ResolveFrame>,
    mut next_frames: Vec<ResolveFrame>,
    strategy: SearchStrategy,
) {
    if matches!(strategy, SearchStrategy::Dfs) {
        next_frames.reverse();
    }
    for frame in next_frames {
        push_frame(frames, frame, strategy);
    }
}
