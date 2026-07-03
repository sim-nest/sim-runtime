//! Eval-policy adapter for running logic queries through a [`sim_kernel::Cx`].

use std::sync::{Arc, Mutex};

use sim_kernel::{
    Cx, Demand, EagerPolicy, Error, EvalPolicy, Expr, PreparedArgs, RawArgs, Result, ShapeMatch,
    Value,
};

use crate::{LogicConfig, LogicDb, query::query_all};

/// Eval policy that treats each evaluated expression as a logic query goal.
///
/// The policy delegates resolution to the crate's existing query resolver and
/// returns the first answer as a symbol-keyed table of captured bindings.
pub struct LogicPolicy {
    db: Arc<Mutex<LogicDb>>,
    config: LogicConfig,
}

impl LogicPolicy {
    /// Creates a policy over `db` with the supplied query configuration.
    pub fn new(db: LogicDb, config: LogicConfig) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            config,
        }
    }

    /// Creates a policy over an existing shared database handle.
    pub fn from_shared(db: Arc<Mutex<LogicDb>>, config: LogicConfig) -> Self {
        Self { db, config }
    }

    /// Returns the shared database handle used by this policy.
    pub fn db(&self) -> Arc<Mutex<LogicDb>> {
        Arc::clone(&self.db)
    }
}

impl EvalPolicy for LogicPolicy {
    fn name(&self) -> &'static str {
        "logic"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        EagerPolicy.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        EagerPolicy.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        let answers = {
            let db = self
                .db
                .lock()
                .map_err(|_| Error::PoisonedLock("logic policy db"))?;
            query_all(cx, &db, &self.config, expr, Some(1))?
        };
        match answers.into_iter().next() {
            Some(answer) => answer_to_value(cx, answer),
            None => cx.factory().nil(),
        }
    }
}

fn answer_to_value(cx: &mut Cx, answer: ShapeMatch) -> Result<Value> {
    let mut entries = Vec::new();
    for (name, value) in answer.captures.values() {
        entries.push((name.clone(), value.clone()));
    }
    for (name, expr) in answer.captures.exprs() {
        entries.push((name.clone(), cx.factory().expr(expr.clone())?));
    }
    cx.factory().table(entries)
}
