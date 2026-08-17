//! Bounded, evidence-carrying class lineage policies.

use std::collections::{BTreeMap, BTreeSet};

/// A loader-neutral view of declared class parents.
pub trait LineageGraph {
    type Node: Clone + Ord;

    /// Returns parents in the order declared by the language adapter.
    fn declared_parents(&self, node: &Self::Node) -> Vec<Self::Node>;
}

/// Independent admission limits for a lineage computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineageBudget {
    pub nodes: usize,
    pub work: usize,
}

/// The precedence rule made impossible by a C3 merge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecedenceConstraint<N> {
    /// The node a remaining parent linearization requires first.
    pub before: N,
    /// The blocked candidate that the remaining linearization requires later.
    pub after: N,
}

/// Exact, inspectable failure evidence from a lineage policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineageError<N> {
    Cycle {
        path: Vec<N>,
    },
    ConflictingPrecedence {
        parents: (N, N),
        constraint: PrecedenceConstraint<N>,
    },
    NodeBudgetExhausted {
        limit: usize,
        required: usize,
    },
    WorkBudgetExhausted {
        limit: usize,
        performed: usize,
    },
}

/// Pluggable class-linearization policy.
pub trait LineagePolicy<G: LineageGraph> {
    fn linearize(
        &self,
        graph: &G,
        root: &G::Node,
        budget: LineageBudget,
    ) -> Result<Vec<G::Node>, LineageError<G::Node>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct C3Policy;

#[derive(Clone, Copy, Debug, Default)]
pub struct DeclaredOrderPolicy;

#[derive(Clone, Copy)]
struct Meter {
    limit: usize,
    used: usize,
}

type Traversal<N> = (Vec<N>, BTreeMap<N, Vec<N>>);

impl Meter {
    fn charge<N>(&mut self) -> Result<(), LineageError<N>> {
        if self.used == self.limit {
            return Err(LineageError::WorkBudgetExhausted {
                limit: self.limit,
                performed: self.used,
            });
        }
        self.used += 1;
        Ok(())
    }
}

fn postorder<G: LineageGraph>(
    graph: &G,
    root: &G::Node,
    budget: LineageBudget,
    work: &mut Meter,
) -> Result<Traversal<G::Node>, LineageError<G::Node>> {
    let mut parents = BTreeMap::new();
    let mut state = BTreeMap::<G::Node, u8>::new();
    let mut path = Vec::new();
    let mut order = Vec::new();
    let mut stack = vec![(root.clone(), false)];

    while let Some((node, leaving)) = stack.pop() {
        work.charge()?;
        if leaving {
            state.insert(node.clone(), 2);
            let popped = path.pop();
            debug_assert!(popped.as_ref() == Some(&node));
            order.push(node);
            continue;
        }
        match state.get(&node).copied() {
            Some(2) => continue,
            Some(1) => {
                let start = path.iter().position(|item| item == &node).unwrap_or(0);
                let mut cycle = path[start..].to_vec();
                cycle.push(node);
                return Err(LineageError::Cycle { path: cycle });
            }
            None => {}
            _ => unreachable!(),
        }
        let required = state.len() + 1;
        if required > budget.nodes {
            return Err(LineageError::NodeBudgetExhausted {
                limit: budget.nodes,
                required,
            });
        }
        state.insert(node.clone(), 1);
        path.push(node.clone());
        let direct = graph.declared_parents(&node);
        parents.insert(node.clone(), direct.clone());
        stack.push((node, true));
        for parent in direct.into_iter().rev() {
            stack.push((parent, false));
        }
    }
    Ok((order, parents))
}

impl<G: LineageGraph> LineagePolicy<G> for DeclaredOrderPolicy {
    fn linearize(
        &self,
        graph: &G,
        root: &G::Node,
        budget: LineageBudget,
    ) -> Result<Vec<G::Node>, LineageError<G::Node>> {
        let mut work = Meter {
            limit: budget.work,
            used: 0,
        };
        let (_, parents) = postorder(graph, root, budget, &mut work)?;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![root.clone()];
        while let Some(node) = stack.pop() {
            work.charge()?;
            if !seen.insert(node.clone()) {
                continue;
            }
            result.push(node.clone());
            for parent in parents.get(&node).into_iter().flatten().rev() {
                stack.push(parent.clone());
            }
        }
        Ok(result)
    }
}

impl<G: LineageGraph> LineagePolicy<G> for C3Policy {
    fn linearize(
        &self,
        graph: &G,
        root: &G::Node,
        budget: LineageBudget,
    ) -> Result<Vec<G::Node>, LineageError<G::Node>> {
        let mut work = Meter {
            limit: budget.work,
            used: 0,
        };
        let (order, parents) = postorder(graph, root, budget, &mut work)?;
        let mut complete = BTreeMap::<G::Node, Vec<G::Node>>::new();
        for node in order {
            let direct = &parents[&node];
            let mut sequences: Vec<Vec<G::Node>> = direct
                .iter()
                .map(|parent| complete[parent].clone())
                .collect();
            sequences.push(direct.clone());
            let mut merged = vec![node.clone()];
            loop {
                sequences.retain(|sequence| !sequence.is_empty());
                if sequences.is_empty() {
                    break;
                }
                let mut selected = None;
                for sequence in &sequences {
                    work.charge()?;
                    let candidate = &sequence[0];
                    if sequences
                        .iter()
                        .all(|other| !other[1..].contains(candidate))
                    {
                        selected = Some(candidate.clone());
                        break;
                    }
                }
                let Some(candidate) = selected else {
                    let before = sequences[0][0].clone();
                    let blocker = sequences
                        .iter()
                        .find(|sequence| sequence[1..].contains(&before))
                        .expect("blocked C3 head");
                    let required_before = blocker[0].clone();
                    return Err(LineageError::ConflictingPrecedence {
                        parents: (before.clone(), required_before.clone()),
                        constraint: PrecedenceConstraint {
                            before: required_before,
                            after: before,
                        },
                    });
                };
                merged.push(candidate.clone());
                for sequence in &mut sequences {
                    if sequence.first() == Some(&candidate) {
                        sequence.remove(0);
                    }
                }
            }
            complete.insert(node, merged);
        }
        Ok(complete.remove(root).expect("root visited"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Graph(BTreeMap<usize, Vec<usize>>);
    impl LineageGraph for Graph {
        type Node = usize;
        fn declared_parents(&self, node: &usize) -> Vec<usize> {
            self.0.get(node).cloned().unwrap_or_default()
        }
    }
    fn generous() -> LineageBudget {
        LineageBudget {
            nodes: 100,
            work: 10_000,
        }
    }

    #[test]
    fn classic_c3_conflict_reports_parents_and_constraint() {
        let graph = Graph(BTreeMap::from([
            (1, vec![]),
            (2, vec![]),
            (3, vec![1, 2]),
            (4, vec![2, 1]),
            (5, vec![3, 4]),
        ]));
        assert_eq!(
            C3Policy.linearize(&graph, &5, generous()),
            Err(LineageError::ConflictingPrecedence {
                parents: (1, 2),
                constraint: PrecedenceConstraint {
                    before: 2,
                    after: 1
                }
            })
        );
    }

    #[test]
    fn ten_thousand_deep_hostile_chain_exhausts_without_recursion() {
        let graph = Graph((1..10_000).map(|node| (node, vec![node - 1])).collect());
        assert_eq!(
            C3Policy.linearize(
                &graph,
                &9_999,
                LineageBudget {
                    nodes: 512,
                    work: 20_000
                }
            ),
            Err(LineageError::NodeBudgetExhausted {
                limit: 512,
                required: 513
            })
        );
    }

    #[test]
    fn exact_cycle_path_is_retained() {
        let graph = Graph(BTreeMap::from([(1, vec![2]), (2, vec![3]), (3, vec![1])]));
        assert_eq!(
            DeclaredOrderPolicy.linearize(&graph, &1, generous()),
            Err(LineageError::Cycle {
                path: vec![1, 2, 3, 1]
            })
        );
    }

    #[test]
    fn both_policies_are_deterministic_across_graph_insertion_orders() {
        let rows = [(0, vec![]), (1, vec![0]), (2, vec![0]), (3, vec![1, 2])];
        let forward = Graph(rows.clone().into_iter().collect());
        let reverse = Graph(rows.into_iter().rev().collect());
        assert_eq!(
            C3Policy.linearize(&forward, &3, generous()).unwrap(),
            vec![3, 1, 2, 0]
        );
        assert_eq!(
            C3Policy.linearize(&forward, &3, generous()),
            C3Policy.linearize(&reverse, &3, generous())
        );
        assert_eq!(
            DeclaredOrderPolicy.linearize(&forward, &3, generous()),
            DeclaredOrderPolicy.linearize(&reverse, &3, generous())
        );
    }
}
