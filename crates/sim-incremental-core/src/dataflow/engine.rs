//! Stable, budgeted worklist propagation.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use crate::{BudgetKind, QueryBudgets};

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
#[derive(Clone, Debug, Eq, PartialEq)]
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
                pending.insert(node_id);
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
                }
            }
        }
        Ok(DataflowSolution {
            states,
            events,
            usage,
        })
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

    #[derive(Clone, Debug, Eq, PartialEq)]
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
}
