use std::sync::{Arc, Mutex};

use sim_kernel::{
    Cx, DefaultFactory, Demand, EagerPolicy, Error, EvalPolicy, Expr, PreparedArgs, RawArgs,
    Result, ShapeMatch, Symbol, Value,
};

use crate::{LogicConfig, LogicDb, query::query_all};

// Probe signatures used by this test:
// - EvalPolicy::eval_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value>
// - query_all(
//     cx: &mut Cx,
//     db: &LogicDb,
//     config: &LogicConfig,
//     goal: Expr,
//     limit: Option<usize>,
//   ) -> Result<Vec<ShapeMatch>>
// - Shape::check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch>
// - ShapeBindings::exprs(&self) -> &[(Symbol, Expr)]
// - SequenceProducer =
//     Arc<dyn Fn(&mut Cx, usize) -> Result<Option<Value>> + Send + Sync + 'static>
// - LazySequence::new(producer: SequenceProducer) -> Self
// - ControlPolicy::enter_prompt(&self, cx: &mut Cx, prompt: &ControlPrompt) -> Result<()>
// - ControlPolicy::capture(&self, cx: &mut Cx, capture: &ControlCapture) -> Result<Ref>
// - ControlPolicy::abort(&self, cx: &mut Cx, abort: &ControlAbort) -> Result<Ref>
// - ControlPolicy::resume(&self, cx: &mut Cx, resume: &ControlResume) -> Result<Ref>

struct ProbeLogicPolicy {
    db: Arc<LogicDb>,
    config: LogicConfig,
    answers: Arc<Mutex<Vec<ShapeMatch>>>,
}

impl ProbeLogicPolicy {
    fn new(db: LogicDb, answers: Arc<Mutex<Vec<ShapeMatch>>>) -> Self {
        Self {
            db: Arc::new(db),
            config: LogicConfig::default(),
            answers,
        }
    }
}

impl EvalPolicy for ProbeLogicPolicy {
    fn name(&self) -> &'static str {
        "probe-logic"
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
        let answers = query_all(cx, &self.db, &self.config, expr, Some(1))?;
        *self
            .answers
            .lock()
            .map_err(|_| Error::PoisonedLock("probe logic answers"))? = answers;
        cx.factory().nil()
    }
}

fn parent_fact() -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("parent")),
            Expr::Symbol(Symbol::new("tom")),
            Expr::Symbol(Symbol::new("bob")),
        ]),
    ])
}

fn parent_query() -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Local(Symbol::new("X")),
    ])
}

#[test]
fn eval_policy_can_route_one_fact_query_to_logic_resolver() {
    let answers = Arc::new(Mutex::new(Vec::new()));
    let mut db = LogicDb::new();
    db.assert_clause_expr(parent_fact()).unwrap();

    let policy = Arc::new(ProbeLogicPolicy::new(db, Arc::clone(&answers)));
    let mut cx = Cx::new(policy, Arc::new(DefaultFactory));
    cx.eval_expr(parent_query()).unwrap();

    let answers = answers.lock().unwrap();
    assert_eq!(answers.len(), 1);
    assert!(answers[0].accepted);
    assert_eq!(
        answers[0].captures.exprs(),
        &[(Symbol::new("X"), Expr::Symbol(Symbol::new("bob")))]
    );
}

#[test]
fn logic_policy_eval_returns_first_answer_bindings() {
    let mut db = LogicDb::new();
    db.assert_clause_expr(parent_fact()).unwrap();

    let policy = Arc::new(crate::LogicPolicy::new(db, LogicConfig::default()));
    assert_eq!(
        policy
            .db()
            .lock()
            .expect("logic policy db lock should be available")
            .clauses()
            .len(),
        1
    );
    let mut cx = Cx::new(policy, Arc::new(DefaultFactory));
    let result = cx.eval_expr(parent_query()).unwrap();

    let table = result
        .object()
        .as_table_impl()
        .expect("logic answer must be a table");
    let x = table.get(&mut cx, Symbol::new("X")).unwrap();
    assert_eq!(
        x.object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(Symbol::new("bob"))
    );
}
