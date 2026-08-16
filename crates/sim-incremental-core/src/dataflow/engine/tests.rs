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

    #[test]
    fn completion_proof_recomputes_only_the_edited_fifty_node_cone() {
        let graph = DataflowGraph::build(
            (0_u8..50).map(node),
            (0_u8..49).map(|id| edge(id, id, id + 1, GraphDirection::Forward, EdgeClass::Data)),
        )
        .unwrap();
        let policy =
            AdmittedTransfer::admit(Identity, &[Facts(0), Facts(1), Facts(2), Facts(3)]).unwrap();
        let budgets = QueryBudgets::default();
        let original =
            FixpointEngine::solve_proven(&graph, &policy, Facts(0), [(0, Facts(1))], budgets)
                .unwrap();

        let edited_seeds = [(0, Facts(1)), (25, Facts(2))];
        assert_eq!(
            FixpointEngine::present(
                &original,
                &graph,
                &policy,
                &Facts(0),
                edited_seeds.clone(),
                budgets,
            ),
            Err(CompletionProofMismatch::Dependencies)
        );
        let incremental = FixpointEngine::solve_incremental(
            &original,
            &graph,
            &policy,
            Facts(0),
            edited_seeds.clone(),
            budgets,
        )
        .unwrap();
        let clean =
            FixpointEngine::solve_proven(&graph, &policy, Facts(0), edited_seeds, budgets).unwrap();

        let visits = incremental
            .solution()
            .events()
            .iter()
            .filter(|event| matches!(event, DataflowEvent::Visit(node) if *node >= 25))
            .count();
        assert_eq!(visits, 25);
        assert_eq!(incremental.solution().usage().work, 74);
        assert_eq!(incremental.identity(), clean.identity());
        assert_eq!(incremental.node_fingerprints(), clean.node_fingerprints());
    }
}
