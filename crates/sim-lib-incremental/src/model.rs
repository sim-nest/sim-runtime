//! Incremental engine session object and SIM expression conversion.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use sim_incremental_core::{
    GraphSnapshot, IncrementalEngine, IncrementalError, Observation, ObservationKind, QueryFrame,
    QueryResult, SnapshotBudgets,
    dataflow::{
        DATAFLOW_PROOF_SCHEMA_REVISION, DataflowCompletionProof, DataflowEvent, DataflowGraph,
        EdgeClass,
    },
};
use sim_kernel::{
    Cx, Dir, Error, Expr, Object, ObjectCompat, Result, Symbol, Table, Value,
    id::CORE_TABLE_CLASS_ID, object::ClassRef,
};

/// Immutable runtime projection of a core dataflow completion proof.
///
/// The constructor consumes a proof minted by `sim-incremental-core`; callers
/// cannot assemble the projection from unverified states or a boolean flag.
/// Its fields are deliberately private, so there is no struct-literal escape
/// hatch for manufacturing a purported completion:
///
/// ```compile_fail
/// use sim_lib_incremental::DataflowAnalysisView;
/// let forged = DataflowAnalysisView { root: todo!() };
/// ```
///
/// Projected states are immutable expressions, rather than handles with a
/// mutation protocol:
///
/// ```compile_fail
/// use sim_kernel::{Cx, Expr, Symbol};
/// fn overwrite(cx: &mut Cx, state: &Expr) {
///     state.set(cx, Symbol::new("forged"), Expr::Nil).unwrap();
/// }
/// ```
pub struct DataflowAnalysisView {
    root: ReadOnlyDir,
}

impl DataflowAnalysisView {
    /// Projects a completed analysis into the standard browse surface.
    ///
    /// `project_state` is presentation-only; the proof identity and all state
    /// ownership remain in the consumed core proof.
    pub fn from_completion<C, S, F>(
        cx: &mut Cx,
        graph: &DataflowGraph<String, String, String, C>,
        proof: DataflowCompletionProof<String, String, C, S>,
        project_state: F,
    ) -> Result<Self>
    where
        C: Clone + std::fmt::Debug + std::hash::Hash + Ord,
        F: Fn(&S) -> Expr,
    {
        let solution = proof.solution();
        let graph_rows = graph
            .nodes()
            .map(|node| {
                let id = string_value(cx, node.id())?;
                let location = string_value(cx, node.location())?;
                let boundary = string_value(cx, &format!("{:?}", node.boundary()))?;
                row(
                    cx,
                    vec![("id", id), ("location", location), ("boundary", boundary)],
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let edge_rows = graph
            .edges()
            .map(|edge| {
                let id = string_value(cx, edge.id())?;
                let source = string_value(cx, edge.source())?;
                let target = string_value(cx, edge.target())?;
                let class = string_value(cx, &edge_class_name(edge.class()))?;
                row(
                    cx,
                    vec![
                        ("id", id),
                        ("source", source),
                        ("target", target),
                        ("class", class),
                    ],
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let input_nodes = graph
            .nodes()
            .filter(|node| {
                matches!(
                    node.boundary(),
                    sim_incremental_core::dataflow::Boundary::Input
                        | sim_incremental_core::dataflow::Boundary::InputOutput
                )
            })
            .map(|node| Symbol::new(node.id().as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let output_nodes = graph
            .nodes()
            .filter(|node| {
                matches!(
                    node.boundary(),
                    sim_incremental_core::dataflow::Boundary::Output
                        | sim_incremental_core::dataflow::Boundary::InputOutput
                )
            })
            .map(|node| Symbol::new(node.id().as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let states = solution
            .states()
            .map(|(node, state)| {
                Ok((
                    Symbol::new(node.as_str()),
                    cx.factory().expr(project_state(state))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let explanations = solution
            .states()
            .map(|(node, _)| {
                let explanation = solution.explain(node, usize::MAX);
                let predecessors = explanation
                    .as_ref()
                    .map(|value| {
                        value
                            .predecessors()
                            .iter()
                            .map(|cause| {
                                let node = string_value(cx, &cause.node)?;
                                let edge = match &cause.edge {
                                    Some(edge) => string_value(cx, edge)?,
                                    None => cx.factory().nil()?,
                                };
                                row(cx, vec![("node", node), ("edge", edge)])
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let omitted = explanation.as_ref().map_or(0, |value| value.omitted());
                let predecessors = list_value(cx, predecessors)?;
                let omitted = number_value(cx, omitted)?;
                Ok((
                    Symbol::new(node.as_str()),
                    readonly_dir_value(
                        cx,
                        vec![("predecessors", predecessors), ("omitted", omitted)],
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let input_states = states
            .iter()
            .filter(|(node, _)| input_nodes.contains(node))
            .cloned()
            .collect();
        let output_states = states
            .into_iter()
            .filter(|(node, _)| output_nodes.contains(node))
            .collect();
        let events = solution
            .events()
            .iter()
            .map(|event| event_value(cx, event))
            .collect::<Result<Vec<_>>>()?;
        let usage = solution.usage();
        let work_count = number_value(cx, usage.work)?;
        let observation_count = number_value(cx, usage.observations)?;
        let depth = number_value(cx, usage.depth)?;
        let output = number_value(cx, usage.output)?;
        let events = list_value(cx, events)?;
        let work = readonly_dir_value(
            cx,
            vec![
                ("work", work_count),
                ("observations", observation_count),
                ("depth", depth),
                ("output", output),
                ("events", events),
            ],
        )?;
        let schema = number_value(cx, DATAFLOW_PROOF_SCHEMA_REVISION as usize)?;
        let fingerprint = unsigned_value(cx, proof.identity().get())?;
        let proof_identity =
            readonly_dir_value(cx, vec![("schema", schema), ("fingerprint", fingerprint)])?;
        let graph_nodes = list_value(cx, graph_rows)?;
        let graph_edges = list_value(cx, edge_rows)?;
        let graph = readonly_dir_value(cx, vec![("nodes", graph_nodes), ("edges", graph_edges)])?;
        let root = ReadOnlyDir::new(vec![
            (Symbol::new("graph"), graph),
            (
                Symbol::new("input-states"),
                readonly_symbol_dir_value(cx, input_states)?,
            ),
            (
                Symbol::new("output-states"),
                readonly_symbol_dir_value(cx, output_states)?,
            ),
            (Symbol::new("work-receipt"), work),
            (
                Symbol::new("explanations"),
                readonly_symbol_dir_value(cx, explanations)?,
            ),
            (Symbol::new("continuation"), cx.factory().nil()?),
            (Symbol::new("proof"), proof_identity),
        ]);
        Ok(Self { root })
    }
}

impl Object for DataflowAnalysisView {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<dataflow-analysis complete>".to_owned())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for DataflowAnalysisView {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        table_class(cx)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.root.as_table_expr(cx)
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for DataflowAnalysisView {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("incremental", "Analysis")
    }
    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.root.get(cx, key)
    }
    fn set(&self, _: &mut Cx, _: Symbol, _: Value) -> Result<()> {
        Err(readonly_error())
    }
    fn has(&self, cx: &mut Cx, key: Symbol) -> Result<bool> {
        self.root.has(cx, key)
    }
    fn del(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn keys(&self, cx: &mut Cx) -> Result<Vec<Symbol>> {
        self.root.keys(cx)
    }
    fn entries(&self, cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        self.root.entries(cx)
    }
    fn len(&self, cx: &mut Cx) -> Result<usize> {
        self.root.len(cx)
    }
    fn clear(&self, _: &mut Cx) -> Result<()> {
        Err(readonly_error())
    }
}

impl Dir for DataflowAnalysisView {
    fn mkdir(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        self.root.opendir(cx, name)
    }
    fn rmdir(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn is_dir(&self, cx: &mut Cx, name: Symbol) -> Result<bool> {
        self.root.is_dir(cx, name)
    }
}

#[derive(Clone)]
struct ReadOnlyDir {
    entries: Arc<Vec<(Symbol, Value)>>,
}

impl ReadOnlyDir {
    fn new(entries: Vec<(Symbol, Value)>) -> Self {
        Self {
            entries: Arc::new(entries),
        }
    }
}
impl Object for ReadOnlyDir {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<read-only-dir>".to_owned())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for ReadOnlyDir {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        table_class(cx)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table_expr(cx)
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}
impl Table for ReadOnlyDir {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("incremental", "ReadOnlyDir")
    }
    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        Ok(self
            .entries
            .iter()
            .find(|(k, _)| k == &key)
            .map(|(_, v)| v.clone())
            .unwrap_or(cx.factory().nil()?))
    }
    fn set(&self, _: &mut Cx, _: Symbol, _: Value) -> Result<()> {
        Err(readonly_error())
    }
    fn has(&self, _: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self.entries.iter().any(|(k, _)| k == &key))
    }
    fn del(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn keys(&self, _: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self.entries.iter().map(|(k, _)| k.clone()).collect())
    }
    fn entries(&self, _: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(self.entries.as_ref().clone())
    }
    fn len(&self, _: &mut Cx) -> Result<usize> {
        Ok(self.entries.len())
    }
    fn clear(&self, _: &mut Cx) -> Result<()> {
        Err(readonly_error())
    }
}
impl Dir for ReadOnlyDir {
    fn mkdir(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        let value = self.get(cx, name)?;
        let is_dir = value.object().as_dir().is_some();
        Ok(is_dir.then_some(value))
    }
    fn rmdir(&self, _: &mut Cx, _: Symbol) -> Result<Value> {
        Err(readonly_error())
    }
    fn is_dir(&self, cx: &mut Cx, name: Symbol) -> Result<bool> {
        Ok(self.opendir(cx, name)?.is_some())
    }
}

fn readonly_dir_value(cx: &mut Cx, entries: Vec<(&str, Value)>) -> Result<Value> {
    readonly_symbol_dir_value(
        cx,
        entries
            .into_iter()
            .map(|(k, v)| (Symbol::new(k), v))
            .collect(),
    )
}
fn readonly_symbol_dir_value(cx: &mut Cx, entries: Vec<(Symbol, Value)>) -> Result<Value> {
    cx.factory().opaque(Arc::new(ReadOnlyDir::new(entries)))
}
fn row(cx: &mut Cx, entries: Vec<(&str, Value)>) -> Result<Value> {
    readonly_dir_value(cx, entries)
}
fn string_value(cx: &mut Cx, value: &str) -> Result<Value> {
    cx.factory().string(value.to_owned())
}
fn list_value(cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
    cx.factory().list(values)
}
fn unsigned_value(cx: &mut Cx, value: u64) -> Result<Value> {
    cx.factory()
        .number_literal(Symbol::qualified("core", "u64"), value.to_string())
}
fn table_class(cx: &mut Cx) -> Result<ClassRef> {
    cx.resolve_class(&Symbol::qualified("core", "Table"))
        .or_else(|_| {
            cx.factory()
                .class_stub(CORE_TABLE_CLASS_ID, Symbol::qualified("core", "Table"))
        })
}
fn readonly_error() -> Error {
    Error::Eval("incremental analysis projections are immutable proof views".to_owned())
}
fn edge_class_name<C: std::fmt::Debug>(class: &EdgeClass<C>) -> String {
    format!("{class:?}")
}

fn event_value<C: std::fmt::Debug>(
    cx: &mut Cx,
    event: &DataflowEvent<String, String, C>,
) -> Result<Value> {
    match event {
        DataflowEvent::Visit(node) => {
            let kind = string_value(cx, "visit")?;
            let node = string_value(cx, node)?;
            row(cx, vec![("kind", kind), ("node", node)])
        }
        DataflowEvent::Propagate { edge, class } => {
            let kind = string_value(cx, "propagate")?;
            let edge = string_value(cx, edge)?;
            let class = string_value(cx, &edge_class_name(class))?;
            row(cx, vec![("kind", kind), ("edge", edge), ("class", class)])
        }
    }
}

/// A SIM runtime object carrying one incremental query graph.
pub struct IncrementalSession {
    engine: Mutex<IncrementalEngine<String, Expr>>,
    store: Arc<Mutex<QueryStore>>,
}

#[derive(Default)]
struct QueryStore {
    sources: BTreeMap<String, Expr>,
    executions: BTreeMap<String, usize>,
}

impl IncrementalSession {
    /// Creates an empty incremental query session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(IncrementalEngine::new()),
            store: Arc::new(Mutex::new(QueryStore::default())),
        }
    }

    /// Registers or replaces a query source expression.
    pub fn register(&self, key: String, source: Expr) -> Result<()> {
        {
            let mut store = self.store.lock().map_err(|_| lock_error())?;
            store.sources.insert(key.clone(), source);
            store.executions.entry(key.clone()).or_insert(0);
        }
        let store = Arc::clone(&self.store);
        self.engine
            .lock()
            .map_err(|_| lock_error())?
            .register_fn(key, move |key, frame| {
                evaluate_registered_query(&store, key, frame)
            });
        Ok(())
    }

    /// Invalidates a query or external observation key.
    pub fn invalidate(&self, key: &str) {
        if let Ok(mut engine) = self.engine.lock() {
            engine.invalidate(&key.to_owned());
        }
    }

    /// Verifies one root query and returns its expression value.
    pub fn verify(&self, key: &str) -> Result<Expr> {
        self.engine
            .lock()
            .map_err(|_| lock_error())?
            .verify(key.to_owned())
            .map_err(incremental_error)
    }

    /// Returns a table explaining memo and source state for `key`.
    pub fn explain(&self, cx: &mut Cx, key: &str) -> Result<Value> {
        let key_string = key.to_owned();
        let engine = self.engine.lock().map_err(|_| lock_error())?;
        let store = self.store.lock().map_err(|_| lock_error())?;
        let dirty = engine.dirty_keys().into_iter().collect::<BTreeSet<_>>();
        let entries = vec![
            symbol_entry(cx, "key", Symbol::new(key))?,
            bool_entry(cx, "registered", store.sources.contains_key(key))?,
            bool_entry(cx, "dirty", dirty.contains(key))?,
            revision_entry(cx, "source-revision", engine.source_revision(&key_string))?,
            optional_revision_entry(cx, "memo-revision", engine.memo_revision(&key_string))?,
            optional_fingerprint_entry(cx, "fingerprint", engine.memo_fingerprint(&key_string))?,
            number_entry(cx, "executions", *store.executions.get(key).unwrap_or(&0))?,
        ];
        cx.factory().table(entries)
    }

    /// Exports a reachable snapshot rooted at `key`.
    pub fn snapshot(&self, cx: &mut Cx, key: &str) -> Result<Value> {
        let snapshot = self
            .engine
            .lock()
            .map_err(|_| lock_error())?
            .snapshot([key.to_owned()], SnapshotBudgets::unlimited())
            .map_err(incremental_error)?;
        snapshot_value(cx, snapshot)
    }

    /// Returns engine metrics and per-query execution counts.
    pub fn metrics(&self, cx: &mut Cx) -> Result<Value> {
        let engine = self.engine.lock().map_err(|_| lock_error())?;
        let store = self.store.lock().map_err(|_| lock_error())?;
        let mut execution_entries = Vec::new();
        for (key, count) in &store.executions {
            execution_entries.push((Symbol::new(key.as_str()), number_value(cx, *count)?));
        }
        let registered = number_entry(cx, "registered", store.sources.len())?;
        let dirty = number_entry(cx, "dirty", engine.dirty_keys().len())?;
        let executions = cx.factory().table(execution_entries)?;
        cx.factory().table(vec![
            registered,
            dirty,
            (Symbol::new("executions"), executions),
        ])
    }
}

impl Default for IncrementalSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Object for IncrementalSession {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<incremental-engine>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IncrementalSession {}

/// Wraps a fresh [`IncrementalSession`] as a runtime value.
pub fn incremental_engine_value(cx: &mut Cx) -> Result<Value> {
    cx.factory().opaque(Arc::new(IncrementalSession::new()))
}

pub(crate) fn require_incremental_engine(value: &Value) -> Result<&IncrementalSession> {
    value
        .object()
        .downcast_ref::<IncrementalSession>()
        .ok_or(Error::TypeMismatch {
            expected: "incremental engine",
            found: "non-engine",
        })
}

fn evaluate_registered_query(
    store: &Arc<Mutex<QueryStore>>,
    key: &String,
    frame: &mut QueryFrame<'_, String, Expr>,
) -> QueryResult<String, Expr> {
    let source = {
        let mut store = store.lock().map_err(|_| IncrementalError::Cancelled)?;
        let count = store.executions.entry(key.clone()).or_insert(0);
        *count = count.saturating_add(1);
        store
            .sources
            .get(key)
            .cloned()
            .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?
    };
    eval_query_expr(&source, frame)
}

fn eval_query_expr(
    expr: &Expr,
    frame: &mut QueryFrame<'_, String, Expr>,
) -> QueryResult<String, Expr> {
    match expr {
        Expr::Call { operator, args } => eval_query_call(operator, args, frame),
        Expr::List(items) => eval_collection(items, frame).map(Expr::List),
        Expr::Vector(items) => eval_collection(items, frame).map(Expr::Vector),
        Expr::Map(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                out.push((eval_query_expr(key, frame)?, eval_query_expr(value, frame)?));
            }
            Ok(Expr::Map(out))
        }
        Expr::Set(items) => eval_collection(items, frame).map(Expr::Set),
        Expr::Infix {
            operator,
            left,
            right,
        } => Ok(Expr::Infix {
            operator: operator.clone(),
            left: Box::new(eval_query_expr(left, frame)?),
            right: Box::new(eval_query_expr(right, frame)?),
        }),
        Expr::Prefix { operator, arg } => Ok(Expr::Prefix {
            operator: operator.clone(),
            arg: Box::new(eval_query_expr(arg, frame)?),
        }),
        Expr::Postfix { operator, arg } => Ok(Expr::Postfix {
            operator: operator.clone(),
            arg: Box::new(eval_query_expr(arg, frame)?),
        }),
        Expr::Block(items) => eval_collection(items, frame).map(Expr::Block),
        Expr::Annotated { expr, annotations } => {
            let mut out = Vec::with_capacity(annotations.len());
            for (key, value) in annotations {
                out.push((key.clone(), eval_query_expr(value, frame)?));
            }
            Ok(Expr::Annotated {
                expr: Box::new(eval_query_expr(expr, frame)?),
                annotations: out,
            })
        }
        Expr::Extension { tag, payload } => Ok(Expr::Extension {
            tag: tag.clone(),
            payload: Box::new(eval_query_expr(payload, frame)?),
        }),
        Expr::Quote { .. } => Ok(expr.clone()),
        _ => Ok(expr.clone()),
    }
}

fn eval_collection(
    items: &[Expr],
    frame: &mut QueryFrame<'_, String, Expr>,
) -> QueryResult<String, Vec<Expr>> {
    items
        .iter()
        .map(|item| eval_query_expr(item, frame))
        .collect()
}

fn eval_query_call(
    operator: &Expr,
    args: &[Expr],
    frame: &mut QueryFrame<'_, String, Expr>,
) -> QueryResult<String, Expr> {
    let Expr::Symbol(symbol) = operator else {
        return Ok(Expr::Call {
            operator: Box::new(eval_query_expr(operator, frame)?),
            args: eval_collection(args, frame)?,
        });
    };
    match symbol.as_qualified_str().as_str() {
        "incremental/read" => {
            let key = one_string_arg("incremental/read", args)?;
            frame.read(key)
        }
        "incremental/missing" => {
            let key = one_string_arg("incremental/missing", args)?;
            frame.observe_missing(key)?;
            Ok(Expr::Bool(false))
        }
        "incremental/listing" => {
            let key = one_string_arg("incremental/listing", args)?;
            frame.observe_listing(key)?;
            Ok(Expr::Bool(true))
        }
        "incremental/policy" => {
            let key = one_string_arg("incremental/policy", args)?;
            frame.observe_policy(key)?;
            Ok(Expr::Bool(true))
        }
        "incremental/epoch" => {
            let key = one_string_arg("incremental/epoch", args)?;
            frame.observe_epoch(key)?;
            Ok(Expr::Bool(true))
        }
        "incremental/vector" => eval_collection(args, frame).map(Expr::Vector),
        "incremental/list" => eval_collection(args, frame).map(Expr::List),
        _ => Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(symbol.clone())),
            args: eval_collection(args, frame)?,
        }),
    }
}

fn one_string_arg(name: &'static str, args: &[Expr]) -> QueryResult<String, String> {
    match args {
        [Expr::String(key)] => Ok(key.clone()),
        _ => Err(IncrementalError::UnknownQuery {
            key: format!("{name} expects one string key"),
        }),
    }
}

fn snapshot_value(cx: &mut Cx, snapshot: GraphSnapshot<String, Expr>) -> Result<Value> {
    let mut nodes = Vec::with_capacity(snapshot.nodes.len());
    for node in snapshot.nodes {
        let dependencies = node
            .dependencies
            .into_iter()
            .map(|observation| observation_value(cx, observation))
            .collect::<Result<Vec<_>>>()?;
        let value = match node.value {
            Some(value) => cx.factory().expr(value)?,
            None => cx.factory().nil()?,
        };
        let fingerprint = match node.fingerprint {
            Some(fingerprint) => number_value(cx, fingerprint.get() as usize)?,
            None => cx.factory().nil()?,
        };
        let key = symbol_entry(cx, "key", Symbol::new(node.key))?;
        let revision = revision_entry(cx, "revision", node.revision)?;
        let dirty = bool_entry(cx, "dirty", node.dirty)?;
        let dependencies = cx.factory().list(dependencies)?;
        nodes.push(cx.factory().table(vec![
            key,
            revision,
            dirty,
            (Symbol::new("value"), value),
            (Symbol::new("fingerprint"), fingerprint),
            (Symbol::new("dependencies"), dependencies),
        ])?);
    }
    let nodes = cx.factory().list(nodes)?;
    cx.factory().table(vec![(Symbol::new("nodes"), nodes)])
}

fn observation_value(cx: &mut Cx, observation: Observation<String>) -> Result<Value> {
    let fingerprint = match observation.fingerprint() {
        Some(fingerprint) => number_value(cx, fingerprint.get() as usize)?,
        None => cx.factory().nil()?,
    };
    let key = symbol_entry(cx, "key", Symbol::new(observation.key().clone()))?;
    let kind = cx.factory().symbol(Symbol::qualified(
        "incremental-observation",
        observation_kind_name(observation.kind()),
    ))?;
    let revision = revision_entry(cx, "revision", observation.revision())?;
    cx.factory().table(vec![
        key,
        (Symbol::new("kind"), kind),
        revision,
        (Symbol::new("fingerprint"), fingerprint),
    ])
}

fn observation_kind_name(kind: &ObservationKind) -> &'static str {
    match kind {
        ObservationKind::Read => "read",
        ObservationKind::Missing => "missing",
        ObservationKind::Listing => "listing",
        ObservationKind::Policy => "policy",
        ObservationKind::Epoch => "epoch",
        ObservationKind::Custom(name) => name,
    }
}

fn incremental_error(err: IncrementalError<String>) -> Error {
    Error::Eval(err.to_string())
}

fn lock_error() -> Error {
    Error::Eval("incremental session lock is poisoned".to_owned())
}

fn symbol_entry(cx: &mut Cx, key: &str, value: Symbol) -> Result<(Symbol, Value)> {
    Ok((Symbol::new(key), cx.factory().symbol(value)?))
}

fn bool_entry(cx: &mut Cx, key: &str, value: bool) -> Result<(Symbol, Value)> {
    Ok((Symbol::new(key), cx.factory().bool(value)?))
}

fn number_entry(cx: &mut Cx, key: &str, value: usize) -> Result<(Symbol, Value)> {
    Ok((Symbol::new(key), number_value(cx, value)?))
}

fn revision_entry(
    cx: &mut Cx,
    key: &str,
    revision: sim_incremental_core::Revision,
) -> Result<(Symbol, Value)> {
    number_entry(cx, key, revision.get() as usize)
}

fn optional_revision_entry(
    cx: &mut Cx,
    key: &str,
    revision: Option<sim_incremental_core::Revision>,
) -> Result<(Symbol, Value)> {
    let value = match revision {
        Some(revision) => number_value(cx, revision.get() as usize)?,
        None => cx.factory().nil()?,
    };
    Ok((Symbol::new(key), value))
}

fn optional_fingerprint_entry(
    cx: &mut Cx,
    key: &str,
    fingerprint: Option<sim_incremental_core::ValueFingerprint>,
) -> Result<(Symbol, Value)> {
    let value = match fingerprint {
        Some(fingerprint) => number_value(cx, fingerprint.get() as usize)?,
        None => cx.factory().nil()?,
    };
    Ok((Symbol::new(key), value))
}

fn number_value(cx: &mut Cx, value: usize) -> Result<Value> {
    cx.factory().number_literal(
        Symbol::qualified("numbers", "u64"),
        (value as u64).to_string(),
    )
}
