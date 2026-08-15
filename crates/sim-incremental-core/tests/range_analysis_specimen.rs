//! Backward range analysis whose ascending loop requires explicit widening.

use sim_incremental_core::{
    BudgetKind, QueryBudgets, ValueFingerprint,
    dataflow::{
        AdmittedTransfer, Boundary, DataflowError, DataflowGraph, DataflowProgress, EdgeClass,
        EdgeSpec, FixpointEngine, GraphDirection, JoinSemilattice, NodeSpec, StateSize,
        TransferPolicy,
    },
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Bound {
    Finite(u64),
    PositiveInfinity,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Range {
    lower: u64,
    upper: Bound,
}

impl Range {
    const fn point(value: u64) -> Self {
        Self {
            lower: value,
            upper: Bound::Finite(value),
        }
    }
}

impl StateSize for Range {
    fn state_size(&self) -> usize {
        size_of::<Self>()
    }
}

impl JoinSemilattice for Range {
    fn bottom(&self) -> Self {
        Self::point(0)
    }

    fn join(&self, other: &Self) -> Self {
        Self {
            lower: self.lower.min(other.lower),
            upper: self.upper.max(other.upper),
        }
    }

    fn less_equal(&self, other: &Self) -> bool {
        other.lower <= self.lower && self.upper <= other.upper
    }
}

/// The caller-owned loop policy. `None` deliberately means no widening.
#[derive(Clone, Copy, Debug)]
struct WideningPolicy {
    widen_after: Option<u64>,
}

impl TransferPolicy<Range> for WideningPolicy {
    fn fingerprint(&self) -> ValueFingerprint {
        ValueFingerprint::new(match self.widen_after {
            Some(threshold) => 0x5749_4445_4e00_0000 ^ threshold,
            None => 0x4e4f_5f57_4944_454e,
        })
    }

    fn policy_size(&self) -> usize {
        size_of::<Self>()
    }

    fn transfer(&self, state: &Range) -> Range {
        let upper = match (state.upper, self.widen_after) {
            (Bound::Finite(value), Some(threshold)) if value >= threshold => {
                Bound::PositiveInfinity
            }
            (Bound::Finite(value), _) => Bound::Finite(value.saturating_add(1)),
            (Bound::PositiveInfinity, _) => Bound::PositiveInfinity,
        };
        Range {
            lower: state.lower,
            upper,
        }
    }
}

#[test]
fn backward_range_analysis_requires_fingerprinted_widening() {
    let graph = range_graph();
    let samples = [
        Range::point(0),
        Range {
            lower: 0,
            upper: Bound::Finite(1),
        },
        Range {
            lower: 0,
            upper: Bound::PositiveInfinity,
        },
    ];
    let no_widening =
        AdmittedTransfer::admit(WideningPolicy { widen_after: None }, &samples).unwrap();
    let seed = [(2, Range::point(1))];

    // The ordinary budgeted engine refuses this infinite ascending chain.
    let refusal = FixpointEngine::solve(
        &graph,
        &no_widening,
        Range::point(0),
        seed,
        QueryBudgets::new(30, usize::MAX, usize::MAX, usize::MAX),
    )
    .unwrap_err();
    assert!(matches!(
        refusal,
        DataflowError::BudgetExceeded {
            kind: BudgetKind::Work,
            ..
        }
    ));

    // The cooperative budget preserves the exact unfinished work behind a token.
    let DataflowProgress::Suspended(continuation) =
        FixpointEngine::start_resumable(&graph, &no_widening, Range::point(0), seed, 5, 2).unwrap()
    else {
        panic!("an unwidened ascending loop must not claim completion");
    };
    assert_ne!(continuation.token().get(), 0);
    let DataflowProgress::Suspended(resumed) = FixpointEngine::resume(
        &graph,
        &no_widening,
        &Range::point(0),
        seed,
        continuation,
        5,
    )
    .unwrap() else {
        panic!("a second finite budget must remain resumable, not hang");
    };
    assert_ne!(resumed.token().get(), 0);

    let widening = AdmittedTransfer::admit(
        WideningPolicy {
            widen_after: Some(3),
        },
        &samples,
    )
    .unwrap();
    assert_ne!(no_widening.fingerprint(), widening.fingerprint());
    let budgets = QueryBudgets::new(100, 100, 3, 1_000);
    let first =
        FixpointEngine::solve_proven(&graph, &widening, Range::point(0), seed, budgets).unwrap();
    let second =
        FixpointEngine::solve_proven(&graph, &widening, Range::point(0), seed, budgets).unwrap();

    assert_eq!(first.identity(), second.identity());
    assert_eq!(
        first.solution().state(&0),
        Some(&Range {
            lower: 0,
            upper: Bound::PositiveInfinity,
        })
    );
    let DataflowProgress::Complete(explained) =
        FixpointEngine::start_resumable(&graph, &widening, Range::point(0), seed, usize::MAX, 1)
            .unwrap()
    else {
        panic!("the widened analysis must converge within one continuation step");
    };
    let explanation = explained.explain(&0, 1).unwrap();
    assert_eq!(explanation.predecessors().len(), 1);
    assert!(explanation.truncated());
    assert!(explanation.omitted() > 0);
}

fn range_graph() -> DataflowGraph<u8, u8, &'static str, ()> {
    DataflowGraph::build(
        [
            NodeSpec {
                id: 0,
                location: "entry",
                boundary: Boundary::Output,
            },
            NodeSpec {
                id: 1,
                location: "loop",
                boundary: Boundary::Internal,
            },
            NodeSpec {
                id: 2,
                location: "exit",
                boundary: Boundary::Input,
            },
        ],
        [
            EdgeSpec {
                id: 0,
                source: 0,
                target: 1,
                class: EdgeClass::Control,
                direction: GraphDirection::Reverse,
            },
            EdgeSpec {
                id: 1,
                source: 1,
                target: 1,
                class: EdgeClass::Data,
                direction: GraphDirection::Reverse,
            },
            EdgeSpec {
                id: 2,
                source: 1,
                target: 2,
                class: EdgeClass::Control,
                direction: GraphDirection::Reverse,
            },
        ],
    )
    .unwrap()
}
