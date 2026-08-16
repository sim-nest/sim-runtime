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
