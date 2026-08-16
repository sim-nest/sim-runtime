/// The operation whose contract failed while solving a graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataflowFailure {
    /// A transfer retracted facts from its input.
    Transfer,
    /// A join was not an upper bound of both operands.
    Join,
}
/// A stable observation from one fixpoint run.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum DataflowEvent<N, E, C> {
    /// A node was removed from the ordered worklist.
    Visit(N),
    /// Facts were propagated across an edge.
    Propagate {
        /// Stable edge identity.
        edge: E,
        /// Semantic edge class, including consumer-defined exceptional classes.
        class: EdgeClass<C>,
    },
}

/// Exact resources consumed by a completed run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DataflowUsage {
    /// Node visits, joins, and transfers performed.
    pub work: usize,
    /// Edge propagations performed.
    pub observations: usize,
    /// Maximum number of simultaneously retained node states.
    pub depth: usize,
    /// State payload units retained over the run.
    pub output: usize,
}

/// A located fixpoint refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataflowError<N, E, L> {
    /// A seed names no node in the immutable graph.
    UnknownSeed(N),
    /// A continuation was presented to inputs other than the ones it captured.
    ContinuationMismatch {
        /// Fingerprint category that changed.
        changed: ContinuationFingerprint,
        /// Fingerprint captured by the continuation.
        expected: ValueFingerprint,
        /// Fingerprint supplied while resuming.
        actual: ValueFingerprint,
    },
    /// A declared budget was exhausted at a node or edge.
    BudgetExceeded {
        /// Exhausted core budget class.
        kind: BudgetKind,
        /// Configured limit.
        limit: usize,
        /// Units that the rejected operation would consume.
        attempted: usize,
        /// Node active at the refusal, when applicable.
        node: Option<N>,
        /// Edge active at the refusal, when applicable.
        edge: Option<E>,
        /// Source/artifact location of `node`, or the edge predecessor.
        location: L,
        /// Edge successor location, when the refusal occurred on an edge.
        target_location: Option<L>,
    },
    /// An admitted transfer violated its contract at a precise node.
    NodeFailure {
        /// Failed operation.
        failure: DataflowFailure,
        /// Stable node identity.
        node: N,
        /// Consumer-neutral node location.
        location: L,
    },
    /// A lattice join violated its contract while propagating a precise edge.
    EdgeFailure {
        /// Failed operation.
        failure: DataflowFailure,
        /// Stable edge identity.
        edge: E,
        /// Edge predecessor location in propagation order.
        location: L,
        /// Edge successor location in propagation order.
        target_location: L,
    },
}

/// Content identity checked before a suspended solve may resume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationFingerprint {
    /// Immutable graph structure and locations.
    Graph,
    /// Admitted transfer semantics and configuration.
    Policy,
    /// Bottom value and canonical seed set.
    Dependencies,
}

/// One causal predecessor retained for a changed node state.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CausalPredecessor<N, E> {
    /// Node whose output caused the change, or the seeded node itself.
    pub node: N,
    /// Propagation edge, absent for a seed fact.
    pub edge: Option<E>,
}

/// A bounded explanation that cannot masquerade as complete after truncation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataflowExplanation<N, E> {
    predecessors: Box<[CausalPredecessor<N, E>]>,
    omitted: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CausalRecord<N, E> {
    retained: Vec<CausalPredecessor<N, E>>,
    omitted: usize,
}

impl<N, E> DataflowExplanation<N, E> {
    /// Returns the retained causal predecessors in deterministic discovery order.
    pub fn predecessors(&self) -> &[CausalPredecessor<N, E>] {
        &self.predecessors
    }

    /// Returns whether causal evidence was omitted by the requested bound.
    pub const fn truncated(&self) -> bool {
        self.omitted != 0
    }

    /// Returns the exact number of causal predecessors omitted by the bound.
    pub const fn omitted(&self) -> usize {
        self.omitted
    }
}

/// A content-bound snapshot of an incomplete deterministic worklist solve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataflowContinuation<N, E, C, S> {
    token: ContinuationToken,
    graph: ValueFingerprint,
    policy: ValueFingerprint,
    dependencies: ValueFingerprint,
    states: BTreeMap<N, S>,
    pending: BTreeSet<N>,
    events: Vec<DataflowEvent<N, E, C>>,
    causes: BTreeMap<N, CausalRecord<N, E>>,
    cause_limit: usize,
    usage: DataflowUsage,
}

impl<N, E, C, S> DataflowContinuation<N, E, C, S> {
    /// Returns the canonical core continuation handle bound to this snapshot.
    pub const fn token(&self) -> ContinuationToken {
        self.token
    }

    /// Returns the graph fingerprint captured by this snapshot.
    pub const fn graph_fingerprint(&self) -> ValueFingerprint {
        self.graph
    }
}

/// Outcome of a resumable solve step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataflowProgress<N, E, C, S> {
    /// The worklist converged.
    Complete(DataflowSolution<N, E, C, S>),
    /// Work remains and is captured without loss.
    Suspended(DataflowContinuation<N, E, C, S>),
}

/// Result of one resumable fixpoint step.
pub type DataflowProgressResult<N, E, L, C, S> =
    Result<DataflowProgress<N, E, C, S>, DataflowError<N, E, L>>;

#[derive(Clone, Copy)]
struct ContinuationIdentity {
    graph: ValueFingerprint,
    policy: ValueFingerprint,
    dependencies: ValueFingerprint,
}

/// Result of one fixpoint solve.
pub type DataflowResult<N, E, L, C, S> =
    Result<DataflowSolution<N, E, C, S>, DataflowError<N, E, L>>;

/// Result of a clean or incremental proof-producing fixpoint solve.
pub type CompletionProofResult<N, E, L, C, S> =
    Result<DataflowCompletionProof<N, E, C, S>, DataflowError<N, E, L>>;

struct ChargeLocation<N, E, L> {
    node: Option<N>,
    edge: Option<E>,
    location: L,
    target_location: Option<L>,
}

/// A converged, fully accounted dataflow solution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataflowSolution<N, E, C, S> {
    states: BTreeMap<N, S>,
    events: Vec<DataflowEvent<N, E, C>>,
    usage: DataflowUsage,
    causes: BTreeMap<N, CausalRecord<N, E>>,
}

/// Schema revision mixed into every completion-proof identity.
pub const DATAFLOW_PROOF_SCHEMA_REVISION: u64 = 1;

/// An immutable witness that a precise set of dataflow inputs reached a fixpoint.
///
/// The witness is deliberately content based: clean and incremental evaluation
/// of the same inputs mint the same identity.  Execution history and visit counts
/// remain diagnostics and cannot change what the proof says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataflowCompletionProof<N, E, C, S> {
    identity: ValueFingerprint,
    graph: ValueFingerprint,
    lattice: ValueFingerprint,
    policy: ValueFingerprint,
    boundaries: ValueFingerprint,
    limits: ValueFingerprint,
    dependencies: ValueFingerprint,
    seed_fingerprints: BTreeMap<N, ValueFingerprint>,
    observations: Box<[(N, E, N)]>,
    node_fingerprints: BTreeMap<N, ValueFingerprint>,
    solution: DataflowSolution<N, E, C, S>,
}

impl<N: Ord, E, C, S> DataflowCompletionProof<N, E, C, S> {
    /// Returns the canonical semantic identity of this completed fixpoint.
    pub const fn identity(&self) -> ValueFingerprint {
        self.identity
    }

    /// Returns the exact dependency edges observed while reaching the fixpoint.
    pub fn observations(&self) -> &[(N, E, N)] {
        &self.observations
    }

    /// Returns the converged node fingerprints used for incremental cutoff.
    pub fn node_fingerprints(&self) -> &BTreeMap<N, ValueFingerprint> {
        &self.node_fingerprints
    }

    /// Returns the proven solution.
    pub const fn solution(&self) -> &DataflowSolution<N, E, C, S> {
        &self.solution
    }
}

/// Why a completion proof cannot be presented for the supplied inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionProofMismatch {
    /// The immutable graph or its boundary declarations changed.
    Graph,
    /// The lattice bottom or state representation changed.
    Lattice,
    /// The admitted transfer policy changed.
    Policy,
    /// The declared resource limits changed.
    Limits,
    /// Entry or external facts changed.
    Dependencies,
}

impl<N: Ord, E, C, S> DataflowSolution<N, E, C, S> {
    /// Returns the converged state for a node.
    pub fn state(&self, node: &N) -> Option<&S> {
        self.states.get(node)
    }
    /// Iterates converged states in stable node order.
    pub fn states(&self) -> impl ExactSizeIterator<Item = (&N, &S)> {
        self.states.iter()
    }
    /// Returns the deterministic visit and propagation sequence.
    pub fn events(&self) -> &[DataflowEvent<N, E, C>] {
        &self.events
    }
    /// Returns exact resource consumption.
    pub const fn usage(&self) -> DataflowUsage {
        self.usage
    }

    /// Explains a node with at most `limit` causal predecessors.
    pub fn explain(&self, node: &N, limit: usize) -> Option<DataflowExplanation<N, E>>
    where
        N: Clone,
        E: Clone,
    {
        let causes = self.causes.get(node)?;
        let retained = causes
            .retained
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        Some(DataflowExplanation {
            omitted: causes
                .omitted
                .saturating_add(causes.retained.len().saturating_sub(retained.len())),
            predecessors: retained.into_boxed_slice(),
        })
    }
}

/// Deterministic worklist solver for admitted monotone analyses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixpointEngine;
