//! Stable, budgeted worklist propagation.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use crate::{BudgetKind, ContinuationToken, FingerprintValue, QueryBudgets, ValueFingerprint};

use super::{AdmittedTransfer, DataflowGraph, EdgeClass, JoinSemilattice, TransferPolicy};

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

impl FixpointEngine {
    /// Solves from explicit entry/exit facts; nodes not reached from a seed stay at bottom.
    pub fn solve<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        budgets: QueryBudgets,
    ) -> DataflowResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice,
        P: TransferPolicy<S>,
    {
        let mut usage = DataflowUsage::default();
        let retained = graph.nodes().count();
        charge(
            &mut usage.depth,
            retained,
            budgets.max_depth,
            BudgetKind::Depth,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        let initial_output = bottom.state_size().saturating_mul(retained);
        charge(
            &mut usage.output,
            initial_output,
            budgets.max_output,
            BudgetKind::Output,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        let mut states = graph
            .nodes()
            .map(|node| (node.id().clone(), bottom.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut pending = BTreeSet::new();
        let mut causes = BTreeMap::<N, CausalRecord<N, E>>::new();
        for (node_id, seed) in seeds {
            let Some(node) = graph.node(&node_id) else {
                return Err(DataflowError::UnknownSeed(node_id));
            };
            let joined = states[&node_id].join(&seed);
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            if !states[&node_id].less_equal(&joined) || !seed.less_equal(&joined) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Join,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            if joined != states[&node_id] {
                replace_state(
                    &mut states,
                    node_id.clone(),
                    joined,
                    &mut usage,
                    budgets,
                    node.location().clone(),
                )?;
                pending.insert(node_id.clone());
                record_cause(
                    &mut causes,
                    node_id.clone(),
                    CausalPredecessor {
                        node: node_id,
                        edge: None,
                    },
                    0,
                );
            }
        }

        let mut events = Vec::new();
        while let Some(node_id) = pending.pop_first() {
            let node = graph
                .node(&node_id)
                .expect("worklist node belongs to graph");
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            events.push(DataflowEvent::Visit(node_id.clone()));
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            let output = transfer.transfer(&states[&node_id]);
            if !states[&node_id].less_equal(&output) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Transfer,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            for edge_id in graph.successors(&node_id).expect("node index is complete") {
                let edge = graph.edge(edge_id).expect("edge index is complete");
                let (_, target_id) = edge.predecessor_and_successor();
                let target = graph.node(target_id).expect("edge target exists");
                charge(
                    &mut usage.observations,
                    1,
                    budgets.max_observations,
                    BudgetKind::Observations,
                    ChargeLocation {
                        node: None,
                        edge: Some(edge_id.clone()),
                        location: node.location().clone(),
                        target_location: Some(target.location().clone()),
                    },
                )?;
                events.push(DataflowEvent::Propagate {
                    edge: edge_id.clone(),
                    class: edge.class().clone(),
                });
                charge_work_edge(
                    &mut usage,
                    budgets,
                    edge_id.clone(),
                    node.location().clone(),
                    target.location().clone(),
                )?;
                let joined = states[target_id].join(&output);
                if !states[target_id].less_equal(&joined) || !output.less_equal(&joined) {
                    return Err(DataflowError::EdgeFailure {
                        failure: DataflowFailure::Join,
                        edge: edge_id.clone(),
                        location: node.location().clone(),
                        target_location: target.location().clone(),
                    });
                }
                if joined != states[target_id] {
                    replace_state(
                        &mut states,
                        target_id.clone(),
                        joined,
                        &mut usage,
                        budgets,
                        target.location().clone(),
                    )?;
                    pending.insert(target_id.clone());
                    record_cause(
                        &mut causes,
                        target_id.clone(),
                        CausalPredecessor {
                            node: node_id.clone(),
                            edge: Some(edge_id.clone()),
                        },
                        0,
                    );
                }
            }
        }
        Ok(DataflowSolution {
            states,
            events,
            usage,
            causes,
        })
    }

    /// Starts a solve and executes at most `max_visits` complete worklist nodes.
    ///
    /// Unlike resource refusal inside [`Self::solve`], this cooperative boundary
    /// snapshots only between nodes, so resumption never repeats callbacks or
    /// presents a shortened event stream as complete.
    pub fn start_resumable<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        max_visits: usize,
        explanation_limit: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        let dependencies = (&bottom, &seeds).incremental_fingerprint();
        let mut states = graph
            .nodes()
            .map(|node| (node.id().clone(), bottom.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut pending = BTreeSet::new();
        let mut causes = BTreeMap::<N, CausalRecord<N, E>>::new();
        for (node_id, seed) in seeds {
            if graph.node(&node_id).is_none() {
                return Err(DataflowError::UnknownSeed(node_id));
            }
            let joined = states[&node_id].join(&seed);
            if joined != states[&node_id] {
                states.insert(node_id.clone(), joined);
                pending.insert(node_id.clone());
                record_cause(
                    &mut causes,
                    node_id.clone(),
                    CausalPredecessor {
                        node: node_id,
                        edge: None,
                    },
                    explanation_limit,
                );
            }
        }
        let continuation = make_continuation(
            ContinuationIdentity {
                graph: graph.fingerprint(),
                policy: transfer.fingerprint(),
                dependencies,
            },
            states,
            pending,
            Vec::new(),
            causes,
            DataflowUsage::default(),
            explanation_limit,
        );
        Self::resume_bound(graph, transfer, continuation, max_visits)
    }

    /// Resumes a content-bound snapshot, refusing every changed input identity.
    pub fn resume<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: &S,
        seeds: impl IntoIterator<Item = (N, S)>,
        continuation: DataflowContinuation<N, E, C, S>,
        max_visits: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        check_fingerprint(
            ContinuationFingerprint::Graph,
            continuation.graph,
            graph.fingerprint(),
        )?;
        check_fingerprint(
            ContinuationFingerprint::Policy,
            continuation.policy,
            transfer.fingerprint(),
        )?;
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        check_fingerprint(
            ContinuationFingerprint::Dependencies,
            continuation.dependencies,
            (bottom, &seeds).incremental_fingerprint(),
        )?;
        Self::resume_bound(graph, transfer, continuation, max_visits)
    }

    fn resume_bound<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        continuation: DataflowContinuation<N, E, C, S>,
        max_visits: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let DataflowContinuation {
            graph: graph_fingerprint,
            policy: policy_fingerprint,
            dependencies,
            mut states,
            mut pending,
            mut events,
            mut causes,
            mut usage,
            cause_limit,
            ..
        } = continuation;
        let mut visits = 0;
        while visits < max_visits {
            let Some(node_id) = pending.pop_first() else {
                return Ok(DataflowProgress::Complete(DataflowSolution {
                    states,
                    events,
                    usage,
                    causes,
                }));
            };
            visits += 1;
            usage.work = usage.work.saturating_add(2);
            events.push(DataflowEvent::Visit(node_id.clone()));
            let output = transfer.transfer(&states[&node_id]);
            let node = graph
                .node(&node_id)
                .expect("snapshot node belongs to graph");
            if !states[&node_id].less_equal(&output) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Transfer,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            for edge_id in graph.successors(&node_id).expect("node index is complete") {
                let edge = graph.edge(edge_id).expect("edge index is complete");
                let (_, target_id) = edge.predecessor_and_successor();
                usage.observations = usage.observations.saturating_add(1);
                usage.work = usage.work.saturating_add(1);
                events.push(DataflowEvent::Propagate {
                    edge: edge_id.clone(),
                    class: edge.class().clone(),
                });
                let joined = states[target_id].join(&output);
                if joined != states[target_id] {
                    usage.output = usage.output.saturating_add(joined.state_size());
                    states.insert(target_id.clone(), joined);
                    pending.insert(target_id.clone());
                    record_cause(
                        &mut causes,
                        target_id.clone(),
                        CausalPredecessor {
                            node: node_id.clone(),
                            edge: Some(edge_id.clone()),
                        },
                        cause_limit,
                    );
                }
            }
        }
        if pending.is_empty() {
            Ok(DataflowProgress::Complete(DataflowSolution {
                states,
                events,
                usage,
                causes,
            }))
        } else {
            Ok(DataflowProgress::Suspended(make_continuation(
                ContinuationIdentity {
                    graph: graph_fingerprint,
                    policy: policy_fingerprint,
                    dependencies,
                },
                states,
                pending,
                events,
                causes,
                usage,
                cause_limit,
            )))
        }
    }
}

fn check_fingerprint<N, E, L>(
    changed: ContinuationFingerprint,
    expected: ValueFingerprint,
    actual: ValueFingerprint,
) -> Result<(), DataflowError<N, E, L>> {
    if expected == actual {
        Ok(())
    } else {
        Err(DataflowError::ContinuationMismatch {
            changed,
            expected,
            actual,
        })
    }
}

fn make_continuation<N, E, C, S>(
    identity: ContinuationIdentity,
    states: BTreeMap<N, S>,
    pending: BTreeSet<N>,
    events: Vec<DataflowEvent<N, E, C>>,
    causes: BTreeMap<N, CausalRecord<N, E>>,
    usage: DataflowUsage,
    cause_limit: usize,
) -> DataflowContinuation<N, E, C, S>
where
    N: Hash + Ord,
    E: Hash,
    C: Hash,
    S: Hash,
{
    let ContinuationIdentity {
        graph,
        policy,
        dependencies,
    } = identity;
    let token = ContinuationToken::new(
        (
            graph,
            policy,
            dependencies,
            &states,
            &pending,
            &events,
            &causes,
            cause_limit,
        )
            .incremental_fingerprint()
            .get(),
    );
    DataflowContinuation {
        token,
        graph,
        policy,
        dependencies,
        states,
        pending,
        events,
        causes,
        cause_limit,
        usage,
    }
}

fn record_cause<N: Ord, E>(
    causes: &mut BTreeMap<N, CausalRecord<N, E>>,
    node: N,
    cause: CausalPredecessor<N, E>,
    limit: usize,
) {
    let record = causes.entry(node).or_insert_with(|| CausalRecord {
        retained: Vec::new(),
        omitted: 0,
    });
    if record.retained.len() < limit {
        record.retained.push(cause);
    } else {
        record.omitted = record.omitted.saturating_add(1);
    }
}

fn replace_state<N: Ord, S: super::StateSize, E, L: Clone>(
    states: &mut BTreeMap<N, S>,
    node: N,
    state: S,
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    location: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.output,
        state.state_size(),
        budgets.max_output,
        BudgetKind::Output,
        ChargeLocation {
            node: None,
            edge: None,
            location,
            target_location: None,
        },
    )?;
    states.insert(node, state);
    Ok(())
}

fn charge_work<N, E, L: Clone>(
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    node: Option<N>,
    location: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.work,
        1,
        budgets.max_work,
        BudgetKind::Work,
        ChargeLocation {
            node,
            edge: None,
            location,
            target_location: None,
        },
    )
}

fn charge_work_edge<N, E, L: Clone>(
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    edge: E,
    location: L,
    target: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.work,
        1,
        budgets.max_work,
        BudgetKind::Work,
        ChargeLocation {
            node: None,
            edge: Some(edge),
            location,
            target_location: Some(target),
        },
    )
}

fn charge<N, E, L>(
    counter: &mut usize,
    units: usize,
    limit: usize,
    kind: BudgetKind,
    at: ChargeLocation<N, E, L>,
) -> Result<(), DataflowError<N, E, L>> {
    let attempted = counter.saturating_add(units);
    if attempted > limit {
        return Err(DataflowError::BudgetExceeded {
            kind,
            limit,
            attempted,
            node: at.node,
            edge: at.edge,
            location: at.location,
            target_location: at.target_location,
        });
    }
    *counter = attempted;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ValueFingerprint,
        dataflow::{Boundary, EdgeSpec, GraphDirection, NodeSpec, StateSize},
    };
    use std::cell::Cell;

    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    struct Facts(u8);

    impl StateSize for Facts {
        fn state_size(&self) -> usize {
            1
        }
    }
    impl JoinSemilattice for Facts {
        fn bottom(&self) -> Self {
            Self(0)
        }
        fn join(&self, other: &Self) -> Self {
            Self(self.0 | other.0)
        }
        fn less_equal(&self, other: &Self) -> bool {
            self.0 & other.0 == self.0
        }
    }
    #[derive(Clone)]
    struct Identity;
    impl TransferPolicy<Facts> for Identity {
        fn fingerprint(&self) -> ValueFingerprint {
            ValueFingerprint::new(1)
        }
        fn policy_size(&self) -> usize {
            0
        }
        fn transfer(&self, state: &Facts) -> Facts {
            state.clone()
        }
    }

    fn node(id: u8) -> NodeSpec<u8, &'static str> {
        NodeSpec {
            id,
            location: match id {
                0 => "entry",
                1 => "loop",
                2 => "exit",
                _ => "dead",
            },
            boundary: Boundary::Internal,
        }
    }
    fn edge(
        id: u8,
        source: u8,
        target: u8,
        direction: GraphDirection,
        class: EdgeClass<&'static str>,
    ) -> EdgeSpec<u8, u8, &'static str> {
        EdgeSpec {
            id,
            source,
            target,
            direction,
            class,
        }
    }

    #[test]
    fn repeated_runs_have_identical_order_and_unreachable_nodes_stay_bottom() {
        let graph = DataflowGraph::build(
            [node(3), node(1), node(0), node(2)],
            [
                edge(
                    2,
                    1,
                    1,
                    GraphDirection::Forward,
                    EdgeClass::Custom("exception"),
                ),
                edge(1, 1, 2, GraphDirection::Forward, EdgeClass::Data),
                edge(0, 0, 2, GraphDirection::Forward, EdgeClass::Control),
                edge(3, 3, 3, GraphDirection::Forward, EdgeClass::Data),
            ],
        )
        .unwrap();
        let policy = AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1), Facts(3)]).unwrap();
        let first = FixpointEngine::solve(
            &graph,
            &policy,
            Facts(0),
            [(0, Facts(1))],
            QueryBudgets::default(),
        )
        .unwrap();
        let second = FixpointEngine::solve(
            &graph,
            &policy,
            Facts(0),
            [(0, Facts(1))],
            QueryBudgets::default(),
        )
        .unwrap();
        assert_eq!(first.events(), second.events());
        assert_eq!(first.state(&3), Some(&Facts(0)));
        assert_eq!(
            first.usage(),
            DataflowUsage {
                work: 6,
                observations: 1,
                depth: 4,
                output: 6
            }
        );
        assert_eq!(
            first.events(),
            &[
                DataflowEvent::Visit(0),
                DataflowEvent::Propagate {
                    edge: 0,
                    class: EdgeClass::Control
                },
                DataflowEvent::Visit(2),
            ]
        );
    }

    #[test]
    fn reverse_and_exceptional_edges_propagate_in_stable_order() {
        let graph = DataflowGraph::build(
            [node(0), node(1), node(2)],
            [
                edge(8, 0, 1, GraphDirection::Reverse, EdgeClass::Custom("throw")),
                edge(9, 2, 1, GraphDirection::Reverse, EdgeClass::Control),
            ],
        )
        .unwrap();
        let policy = AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1)]).unwrap();
        let result = FixpointEngine::solve(
            &graph,
            &policy,
            Facts(0),
            [(1, Facts(1))],
            QueryBudgets::default(),
        )
        .unwrap();
        assert_eq!(result.state(&0), Some(&Facts(1)));
        assert_eq!(result.state(&2), Some(&Facts(1)));
        assert_eq!(
            result.events(),
            &[
                DataflowEvent::Visit(1),
                DataflowEvent::Propagate {
                    edge: 8,
                    class: EdgeClass::Custom("throw")
                },
                DataflowEvent::Propagate {
                    edge: 9,
                    class: EdgeClass::Control
                },
                DataflowEvent::Visit(0),
                DataflowEvent::Visit(2),
            ]
        );
    }

    #[derive(Clone)]
    struct Retract(Cell<bool>);
    impl TransferPolicy<Facts> for Retract {
        fn fingerprint(&self) -> ValueFingerprint {
            ValueFingerprint::new(2)
        }
        fn policy_size(&self) -> usize {
            0
        }
        fn transfer(&self, state: &Facts) -> Facts {
            if self.0.get() {
                Facts(0)
            } else {
                state.clone()
            }
        }
    }

    #[test]
    fn budget_and_contract_failures_carry_precise_locations() {
        let graph = DataflowGraph::build(
            [node(0), node(2)],
            [edge(7, 0, 2, GraphDirection::Forward, EdgeClass::Data)],
        )
        .unwrap();
        let policy = AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1)]).unwrap();
        let error = FixpointEngine::solve(
            &graph,
            &policy,
            Facts(0),
            [(0, Facts(1))],
            QueryBudgets::new(usize::MAX, 0, usize::MAX, usize::MAX),
        )
        .unwrap_err();
        assert_eq!(
            error,
            DataflowError::BudgetExceeded {
                kind: BudgetKind::Observations,
                limit: 0,
                attempted: 1,
                node: None,
                edge: Some(7),
                location: "entry",
                target_location: Some("exit")
            }
        );

        let retracted =
            AdmittedTransfer::admit(Retract(Cell::new(false)), &[Facts(0), Facts(1)]).unwrap();
        retracted.policy().0.set(true);
        let error = FixpointEngine::solve(
            &graph,
            &retracted,
            Facts(0),
            [(0, Facts(1))],
            QueryBudgets::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            DataflowError::NodeFailure {
                failure: DataflowFailure::Transfer,
                node: 0,
                location: "entry"
            }
        );
    }

    #[test]
    fn continuation_resumes_exact_worklist_and_refuses_edited_graph() {
        let graph = DataflowGraph::build(
            [node(0), node(1), node(2)],
            [
                edge(0, 0, 1, GraphDirection::Forward, EdgeClass::Data),
                edge(1, 1, 2, GraphDirection::Forward, EdgeClass::Data),
            ],
        )
        .unwrap();
        let policy = AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1)]).unwrap();
        let progress =
            FixpointEngine::start_resumable(&graph, &policy, Facts(0), [(0, Facts(1))], 1, 8)
                .unwrap();
        let DataflowProgress::Suspended(continuation) = progress else {
            panic!("one visit must leave the chain incomplete");
        };
        assert_ne!(continuation.token().get(), 0);

        let edited = DataflowGraph::build(
            [node(0), node(1), node(2)],
            [
                edge(0, 0, 1, GraphDirection::Forward, EdgeClass::Data),
                edge(1, 1, 2, GraphDirection::Forward, EdgeClass::Data),
                edge(2, 0, 2, GraphDirection::Forward, EdgeClass::Control),
            ],
        )
        .unwrap();
        let error = FixpointEngine::resume(
            &edited,
            &policy,
            &Facts(0),
            [(0, Facts(1))],
            continuation.clone(),
            usize::MAX,
        )
        .unwrap_err();
        assert_eq!(
            error,
            DataflowError::ContinuationMismatch {
                changed: ContinuationFingerprint::Graph,
                expected: continuation.graph_fingerprint(),
                actual: edited.fingerprint(),
            }
        );

        let resumed = FixpointEngine::resume(
            &graph,
            &policy,
            &Facts(0),
            [(0, Facts(1))],
            continuation,
            usize::MAX,
        )
        .unwrap();
        let DataflowProgress::Complete(solution) = resumed else {
            panic!("unbounded resume must converge");
        };
        assert_eq!(solution.state(&2), Some(&Facts(1)));
    }

    #[test]
    fn continuation_refuses_changed_dependencies_and_explanation_reports_omissions() {
        let graph = DataflowGraph::build(
            [node(0), node(1), node(2)],
            [
                edge(0, 0, 2, GraphDirection::Forward, EdgeClass::Data),
                edge(1, 1, 2, GraphDirection::Forward, EdgeClass::Control),
            ],
        )
        .unwrap();
        let policy = AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1), Facts(2)]).unwrap();
        let DataflowProgress::Suspended(continuation) = FixpointEngine::start_resumable(
            &graph,
            &policy,
            Facts(0),
            [(0, Facts(1)), (1, Facts(2))],
            1,
            1,
        )
        .unwrap() else {
            panic!("one seed must remain pending");
        };
        let changed_dependencies = FixpointEngine::resume(
            &graph,
            &policy,
            &Facts(0),
            [(0, Facts(1))],
            continuation.clone(),
            usize::MAX,
        )
        .unwrap_err();
        assert!(matches!(
            changed_dependencies,
            DataflowError::ContinuationMismatch {
                changed: ContinuationFingerprint::Dependencies,
                ..
            }
        ));

        let DataflowProgress::Complete(solution) = FixpointEngine::resume(
            &graph,
            &policy,
            &Facts(0),
            [(0, Facts(1)), (1, Facts(2))],
            continuation,
            usize::MAX,
        )
        .unwrap() else {
            panic!("resume must converge");
        };
        let explanation = solution.explain(&2, 1).unwrap();
        assert_eq!(explanation.predecessors().len(), 1);
        assert!(explanation.truncated());
        assert_eq!(explanation.omitted(), 1);
        assert_eq!(solution.explain(&2, 8).unwrap().omitted(), 1);
    }
}
