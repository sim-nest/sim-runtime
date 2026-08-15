//! Stable tagged-Thompson compilation for validated pattern IR.

use crate::{Anchor, AssertionId, CaptureId, IrNode, PatternIr, SymbolDomain, TextClass};
use core::fmt;
use std::collections::BTreeMap;

/// Stable identifier of an automaton state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId(pub u32);

/// The action performed by one automaton state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction<S, E> {
    /// Accept the pattern.
    Accept,
    /// Continue without consuming input.
    Epsilon {
        /// Successor state.
        next: StateId,
    },
    /// Consume one exact symbol.
    Symbol {
        /// Required symbol.
        symbol: S,
        /// Successor after consumption.
        next: StateId,
    },
    /// Consume any one symbol.
    Any {
        /// Successor after consumption.
        next: StateId,
    },
    /// Delegate symbol recognition to an admitted extension.
    Extension {
        /// Admitted extension matcher.
        extension: E,
        /// Successor after consumption.
        next: StateId,
    },
    /// Choose either successor without consuming input.
    Split {
        /// Ordered branch entry states.
        alternatives: Vec<StateId>,
    },
    /// Record a capture boundary without consuming input.
    Tag {
        /// Stable capture identifier.
        capture: CaptureId,
        /// Boundary being recorded.
        boundary: TagBoundary,
        /// Successor state.
        next: StateId,
    },
    /// Test a subject boundary without consuming input.
    Anchor {
        /// Boundary predicate.
        anchor: Anchor,
        /// Successor when the predicate holds.
        next: StateId,
    },
    /// Invoke a separately compiled zero-width assertion.
    Assertion {
        /// Stable assertion identifier.
        assertion: AssertionId,
        /// Successor when the assertion holds.
        next: StateId,
    },
    /// Enter a bounded or unbounded repetition loop.
    Repeat {
        /// Repeated body entry.
        body: StateId,
        /// Loop exit.
        exit: StateId,
        /// Required iteration count.
        min: usize,
        /// Maximum iteration count, or no bound.
        max: Option<usize>,
        /// Whether the body precedes the exit in execution priority.
        greedy: bool,
    },
}

/// A capture-tag boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagBoundary {
    /// Opening boundary.
    Start,
    /// Closing boundary.
    End,
}

/// One stable state in a compiled automaton.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State<S, E> {
    /// Stable, dense identifier equal to this state's position.
    pub id: StateId,
    /// State behavior and outgoing edges.
    pub instruction: Instruction<S, E>,
}

/// Browsable facts about a compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompilationEvidence {
    /// Total number of graph states.
    pub state_count: usize,
    /// Number of capture tags in the graph.
    pub capture_count: usize,
    /// Conservative work per subject position for an NFA state-set executor.
    pub estimated_work_per_symbol: usize,
}

/// A compiled, explicitly branching tagged automaton.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Automaton<S, E> {
    start: StateId,
    states: Vec<State<S, E>>,
    evidence: CompilationEvidence,
    assertions: BTreeMap<AssertionId, AssertionProgram<S, E>>,
}

/// A separately compiled lookahead whose width is known before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertionProgram<S, E> {
    automaton: Box<Automaton<S, E>>,
    width: usize,
}

impl<S, E> AssertionProgram<S, E> {
    /// Compiled regular program used by the zero-width assertion.
    pub fn automaton(&self) -> &Automaton<S, E> {
        &self.automaton
    }

    /// Exact number of subject symbols inspected by the assertion.
    pub const fn width(&self) -> usize {
        self.width
    }
}

impl<S, E> Automaton<S, E> {
    /// Entry state.
    pub const fn start(&self) -> StateId {
        self.start
    }
    /// Dense state table.
    pub fn states(&self) -> &[State<S, E>] {
        &self.states
    }
    /// Compilation size and work estimate.
    pub const fn evidence(&self) -> CompilationEvidence {
        self.evidence
    }

    /// Returns a compiled fixed-width assertion when its definition is regular.
    pub fn assertion(&self, id: AssertionId) -> Option<&AssertionProgram<S, E>> {
        self.assertions.get(&id)
    }
}

/// Compile validated IR into a stable tagged Thompson graph.
pub fn compile<D, E>(ir: &PatternIr<D, E>) -> Automaton<D::Symbol, E>
where
    D: SymbolDomain,
    D::Symbol: Clone,
    E: Clone + fmt::Debug + Ord,
{
    let mut builder = Builder { states: Vec::new() };
    let accept = builder.push(Instruction::Accept);
    let start = builder.node(ir.root(), accept);
    let capture_count = builder
        .states
        .iter()
        .filter(|state| {
            matches!(
                state.instruction,
                Instruction::Tag {
                    boundary: TagBoundary::Start,
                    ..
                }
            )
        })
        .count();
    let state_count = builder.states.len();
    let assertions: BTreeMap<AssertionId, AssertionProgram<D::Symbol, E>> = ir
        .assertions()
        .iter()
        .filter_map(|(id, node)| {
            fixed_width(node).map(|width| {
                (
                    *id,
                    AssertionProgram {
                        automaton: Box::new(compile_node(node)),
                        width,
                    },
                )
            })
        })
        .collect();
    let assertion_state_count = assertions
        .values()
        .map(|program| program.automaton.evidence.state_count)
        .sum::<usize>();
    Automaton {
        start,
        states: builder.states,
        evidence: CompilationEvidence {
            state_count: state_count + assertion_state_count,
            capture_count,
            estimated_work_per_symbol: state_count + assertion_state_count,
        },
        assertions,
    }
}

fn compile_node<S: Clone, E: Clone>(node: &IrNode<S, E>) -> Automaton<S, E> {
    let mut builder = Builder { states: Vec::new() };
    let accept = builder.push(Instruction::Accept);
    let start = builder.node(node, accept);
    let state_count = builder.states.len();
    Automaton {
        start,
        states: builder.states,
        evidence: CompilationEvidence {
            state_count,
            capture_count: 0,
            estimated_work_per_symbol: state_count,
        },
        assertions: BTreeMap::new(),
    }
}

fn fixed_width<S, E>(node: &IrNode<S, E>) -> Option<usize> {
    match node {
        IrNode::Symbol(_) | IrNode::Any | IrNode::Extension(_) => Some(1),
        IrNode::Anchor(_) => Some(0),
        // Nested assertion composition needs its referenced definition to prove
        // width. Keep it in the typed extension lane until compilation carries
        // that dependency closure explicitly.
        IrNode::Assertion(_) => None,
        IrNode::Concat(nodes) => nodes
            .iter()
            .try_fold(0usize, |sum, node| sum.checked_add(fixed_width(node)?)),
        IrNode::Alternation(nodes) => {
            let mut widths = nodes.iter().map(fixed_width);
            let first = widths.next()??;
            widths.all(|width| width == Some(first)).then_some(first)
        }
        IrNode::Repeat { node, bounds, .. } if bounds.max() == Some(bounds.min()) => {
            fixed_width(node)?.checked_mul(bounds.min())
        }
        IrNode::Group(node) | IrNode::Capture { node, .. } => fixed_width(node),
        IrNode::Repeat { .. } => None,
    }
}

struct Builder<S, E> {
    states: Vec<State<S, E>>,
}

impl<S: Clone, E: Clone> Builder<S, E> {
    fn push(&mut self, instruction: Instruction<S, E>) -> StateId {
        let id =
            StateId(u32::try_from(self.states.len()).expect("pattern state count exceeds u32"));
        self.states.push(State { id, instruction });
        id
    }

    fn node(&mut self, node: &IrNode<S, E>, next: StateId) -> StateId {
        match node {
            IrNode::Symbol(symbol) => self.push(Instruction::Symbol {
                symbol: symbol.clone(),
                next,
            }),
            IrNode::Any => self.push(Instruction::Any { next }),
            IrNode::Concat(nodes) => nodes
                .iter()
                .rev()
                .fold(next, |tail, node| self.node(node, tail)),
            IrNode::Alternation(nodes) => {
                let alternatives = nodes.iter().map(|node| self.node(node, next)).collect();
                self.push(Instruction::Split { alternatives })
            }
            IrNode::Repeat {
                node,
                bounds,
                greedy,
            } => {
                let repeat = self.push(Instruction::Epsilon { next });
                let body = self.node(node, repeat);
                self.states[repeat.0 as usize].instruction = Instruction::Repeat {
                    body,
                    exit: next,
                    min: bounds.min(),
                    max: bounds.max(),
                    greedy: *greedy,
                };
                repeat
            }
            IrNode::Group(node) => self.node(node, next),
            IrNode::Capture { id, node } => {
                let end = self.push(Instruction::Tag {
                    capture: *id,
                    boundary: TagBoundary::End,
                    next,
                });
                let body = self.node(node, end);
                self.push(Instruction::Tag {
                    capture: *id,
                    boundary: TagBoundary::Start,
                    next: body,
                })
            }
            IrNode::Anchor(anchor) => self.push(Instruction::Anchor {
                anchor: *anchor,
                next,
            }),
            IrNode::Assertion(assertion) => self.push(Instruction::Assertion {
                assertion: *assertion,
                next,
            }),
            IrNode::Extension(extension) => self.push(Instruction::Extension {
                extension: extension.clone(),
                next,
            }),
        }
    }
}

impl Instruction<char, TextClass> {
    /// Tests whether this instruction consumes `symbol`, reusing `TextClass` membership.
    pub fn matches(&self, symbol: char) -> bool {
        match self {
            Self::Symbol {
                symbol: expected, ..
            } => *expected == symbol,
            Self::Any { .. } => true,
            Self::Extension { extension, .. } => extension.matches(symbol),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ByteDomain, EnginePolicy, RepeatBounds, ScalarDomain};
    use std::collections::BTreeMap;

    fn byte_ir(root: IrNode<u8, &'static str>) -> PatternIr<ByteDomain, &'static str> {
        PatternIr::new(root, BTreeMap::new(), &EnginePolicy::new([])).unwrap()
    }

    #[test]
    fn state_growth_is_linear_across_regular_constructs() {
        let patterns = [
            IrNode::Symbol(b'a'),
            IrNode::Concat(vec![
                IrNode::Symbol(b'a'),
                IrNode::Any,
                IrNode::Anchor(Anchor::SubjectEnd),
            ]),
            IrNode::Alternation(vec![IrNode::Symbol(b'a'), IrNode::Symbol(b'b')]),
            IrNode::Alternation(vec![
                IrNode::Alternation(vec![IrNode::Symbol(b'a'), IrNode::Symbol(b'b')]),
                IrNode::Alternation(vec![IrNode::Symbol(b'c'), IrNode::Symbol(b'd')]),
            ]),
            IrNode::Repeat {
                node: Box::new(IrNode::Group(Box::new(IrNode::Symbol(b'x')))),
                bounds: RepeatBounds::new(3, None).unwrap(),
                greedy: true,
            },
        ];
        for root in patterns {
            let automaton = compile(&byte_ir(root.clone()));
            assert!(automaton.evidence().state_count <= 2 * node_count(&root) + 1);
            assert_eq!(
                automaton.evidence().estimated_work_per_symbol,
                automaton.states().len()
            );
        }
    }

    #[test]
    fn capture_and_state_ids_survive_recompilation() {
        let ir = byte_ir(IrNode::Capture {
            id: CaptureId(41),
            node: Box::new(IrNode::Alternation(vec![
                IrNode::Symbol(b'a'),
                IrNode::Symbol(b'b'),
            ])),
        });
        let first = compile(&ir);
        assert_eq!(first, compile(&ir));
        assert!(first.states().iter().any(|state| matches!(
            state.instruction,
            Instruction::Tag {
                capture: CaptureId(41),
                boundary: TagBoundary::Start,
                ..
            }
        )));
        assert!(
            first
                .states()
                .iter()
                .enumerate()
                .all(|(index, state)| state.id.0 as usize == index)
        );
    }

    #[test]
    fn text_classes_reuse_shared_membership() {
        let ir = PatternIr::<ScalarDomain, TextClass>::new(
            IrNode::Extension(TextClass::Digit),
            BTreeMap::new(),
            &EnginePolicy::new([TextClass::Digit]),
        )
        .unwrap();
        let automaton = compile(&ir);
        let instruction = &automaton.states()[automaton.start().0 as usize].instruction;
        assert!(instruction.matches('7'));
        assert!(!instruction.matches('x'));
    }

    fn node_count<S, E>(node: &IrNode<S, E>) -> usize {
        1 + match node {
            IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
                nodes.iter().map(node_count).sum()
            }
            IrNode::Repeat { node, .. } | IrNode::Group(node) | IrNode::Capture { node, .. } => {
                node_count(node)
            }
            _ => 0,
        }
    }
}
