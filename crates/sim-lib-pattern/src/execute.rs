//! Iterative, resource-accounted execution of regular pattern automata.

use crate::{Anchor, Automaton, CaptureId, Instruction, StateId, TagBoundary, TextLimits};
use std::collections::{BTreeMap, BTreeSet};

/// One completed tagged capture, expressed in subject-symbol offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSpan {
    /// Inclusive start offset.
    pub start: usize,
    /// Exclusive end offset.
    pub end: usize,
}

/// A successful regular-engine match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionMatch {
    /// Inclusive start offset.
    pub start: usize,
    /// Exclusive end offset.
    pub end: usize,
    /// Captures keyed by their stable compiled identifier.
    pub captures: BTreeMap<CaptureId, CaptureSpan>,
}

/// The resource whose configured limit stopped execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionLimit {
    /// The compiled graph contains more states than admitted.
    States,
    /// Transition work reached `TextLimits::max_steps`.
    Transitions,
    /// Capture history reached `TextLimits::max_capture_history`.
    CaptureHistory,
    /// The subject exceeds `TextLimits::max_subject_symbols`.
    Subject,
}

/// Exact work consumed by an execution attempt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecutionReceipt {
    /// Compiled states in the input graph.
    pub state_count: usize,
    /// State configurations removed from the iterative worklists.
    pub state_visits: usize,
    /// Graph transitions considered.
    pub transitions: usize,
    /// Capture-boundary records created.
    pub capture_history: usize,
    /// Subject symbols presented to the executor.
    pub subject_symbols: usize,
}

/// A pattern feature deliberately excluded from the regular executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedFeature {
    /// The assertion has no statically provable fixed width.
    VariableWidthAssertion(crate::AssertionId),
}

/// A typed execution result. Resource exhaustion is never collapsed into rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// The automaton accepted a subject prefix.
    Match {
        /// Match and captures.
        matched: ExecutionMatch,
        /// Consumed work.
        receipt: ExecutionReceipt,
    },
    /// The automaton definitively rejected the subject.
    NoMatch {
        /// Consumed work.
        receipt: ExecutionReceipt,
    },
    /// A configured resource boundary stopped execution.
    Limit {
        /// Exhausted resource.
        limit: ExecutionLimit,
        /// Work consumed before stopping.
        receipt: ExecutionReceipt,
    },
    /// The requested construct belongs to the separately budgeted extension lane.
    Unsupported {
        /// Exact unsupported construct.
        feature: UnsupportedFeature,
        /// Regular work consumed before discovering it.
        receipt: ExecutionReceipt,
    },
}

#[derive(Clone, Debug, Default)]
struct History {
    open: BTreeMap<CaptureId, usize>,
    closed: BTreeMap<CaptureId, CaptureSpan>,
}

#[derive(Clone, Debug)]
struct Thread {
    state: StateId,
    repeats: BTreeMap<StateId, usize>,
    history: History,
}

type SpanMatcher<'a, S, E> = dyn Fn(&E, &[S], usize) -> Option<usize> + 'a;

/// Executes a compiled regular automaton without recursion or backtracking.
///
/// `extension_matches` supplies the consuming predicate for admitted extension
/// states. Fixed-width assertions remain in this accounted regular model;
/// variable-width assertions are returned as typed refusals.
pub fn execute_regular<S, E>(
    automaton: &Automaton<S, E>,
    subject: &[S],
    limits: TextLimits,
    extension_matches: impl Fn(&E, &S) -> bool,
) -> ExecutionOutcome
where
    S: PartialEq,
{
    execute_spanning(
        automaton,
        subject,
        limits,
        |extension, subject, position| {
            subject
                .get(position)
                .filter(|symbol| extension_matches(extension, symbol))
                .map(|_| position + 1)
        },
    )
}

/// Executes an automaton whose admitted extensions may consume any bounded
/// subject span, including a zero-width span.
///
/// The callback returns the exclusive end position of a successful extension
/// match. Returning a position before the supplied start or beyond the subject
/// rejects that extension attempt.
pub(crate) fn execute_spanning<S, E>(
    automaton: &Automaton<S, E>,
    subject: &[S],
    limits: TextLimits,
    extension_match: impl Fn(&E, &[S], usize) -> Option<usize>,
) -> ExecutionOutcome
where
    S: PartialEq,
{
    execute_regular_inner(automaton, subject, limits, &extension_match)
}

fn execute_regular_inner<S, E>(
    automaton: &Automaton<S, E>,
    subject: &[S],
    limits: TextLimits,
    extension_match: &SpanMatcher<'_, S, E>,
) -> ExecutionOutcome
where
    S: PartialEq,
{
    let mut receipt = ExecutionReceipt {
        state_count: automaton.evidence().state_count,
        subject_symbols: subject.len(),
        ..ExecutionReceipt::default()
    };
    if receipt.state_count > limits.max_states {
        return limited(ExecutionLimit::States, receipt);
    }
    if receipt.subject_symbols > limits.max_subject_symbols {
        return limited(ExecutionLimit::Subject, receipt);
    }

    let mut current = vec![(
        0,
        Thread {
            state: automaton.start(),
            repeats: BTreeMap::new(),
            history: History::default(),
        },
    )];
    let mut seen = BTreeSet::new();
    while let Some((position, thread)) = current.pop() {
        receipt.state_visits += 1;
        // Thompson state-set execution retains the first (priority-ordered)
        // history reaching a state at a subject position.
        if !seen.insert((position, thread.state)) {
            continue;
        }
        let Some(state) = automaton.states().get(thread.state.0 as usize) else {
            continue;
        };
        match &state.instruction {
            Instruction::Accept => {
                return ExecutionOutcome::Match {
                    matched: ExecutionMatch {
                        start: 0,
                        end: position,
                        captures: thread.history.closed,
                    },
                    receipt,
                };
            }
            Instruction::Symbol { symbol, next } => {
                if subject.get(position).is_some_and(|found| found == symbol) {
                    push_at(&mut current, position + 1, thread, *next);
                }
            }
            Instruction::Any { next } => {
                if position < subject.len() {
                    push_at(&mut current, position + 1, thread, *next);
                }
            }
            Instruction::Extension { extension, next } => {
                if let Some(end) = extension_match(extension, subject, position)
                    && (position..=subject.len()).contains(&end)
                {
                    push_at(&mut current, end, thread, *next);
                }
            }
            Instruction::Epsilon { next } => push_at(&mut current, position, thread, *next),
            Instruction::Split { alternatives } => {
                for next in alternatives.iter().rev() {
                    push_at(&mut current, position, thread.clone(), *next);
                }
            }
            Instruction::Tag {
                capture,
                boundary,
                next,
            } => {
                if receipt.capture_history == limits.max_capture_history {
                    return limited(ExecutionLimit::CaptureHistory, receipt);
                }
                receipt.capture_history += 1;
                let mut thread = thread;
                match boundary {
                    TagBoundary::Start => {
                        thread.history.open.insert(*capture, position);
                    }
                    TagBoundary::End => {
                        if let Some(start) = thread.history.open.remove(capture) {
                            thread.history.closed.insert(
                                *capture,
                                CaptureSpan {
                                    start,
                                    end: position,
                                },
                            );
                        }
                    }
                }
                push_at(&mut current, position, thread, *next);
            }
            Instruction::Anchor { anchor, next } => {
                let holds = match anchor {
                    Anchor::SubjectStart => position == 0,
                    Anchor::SubjectEnd => position == subject.len(),
                };
                if holds {
                    push_at(&mut current, position, thread, *next);
                }
            }
            Instruction::Repeat {
                body,
                exit,
                min,
                max,
                greedy,
            } => {
                let count = thread.repeats.get(&thread.state).copied().unwrap_or(0);
                let can_repeat = max.is_none_or(|maximum| count < maximum);
                let can_exit = count >= *min;
                let mut body_thread = thread.clone();
                body_thread.repeats.insert(thread.state, count + 1);
                let choices = if *greedy {
                    [(can_exit, *exit, thread), (can_repeat, *body, body_thread)]
                } else {
                    [(can_repeat, *body, body_thread), (can_exit, *exit, thread)]
                };
                for (enabled, next, thread) in choices {
                    if enabled {
                        push_at(&mut current, position, thread, next);
                    }
                }
            }
            Instruction::Assertion { assertion, next } => {
                let Some(program) = automaton.assertion(*assertion) else {
                    return ExecutionOutcome::Unsupported {
                        feature: UnsupportedFeature::VariableWidthAssertion(*assertion),
                        receipt,
                    };
                };
                let end = position.saturating_add(program.width());
                if let Some(window) = subject.get(position..end) {
                    let remaining = TextLimits {
                        max_steps: limits.max_steps.saturating_sub(receipt.transitions),
                        max_states: limits.max_states,
                        max_capture_history: limits
                            .max_capture_history
                            .saturating_sub(receipt.capture_history),
                        max_subject_symbols: limits.max_subject_symbols,
                    };
                    match execute_regular_inner(
                        program.automaton(),
                        window,
                        remaining,
                        extension_match,
                    ) {
                        ExecutionOutcome::Match {
                            matched,
                            receipt: nested,
                        } if matched.end == window.len() => {
                            receipt.state_visits += nested.state_visits;
                            receipt.transitions += nested.transitions;
                            receipt.capture_history += nested.capture_history;
                            push_at(&mut current, position, thread, *next);
                        }
                        ExecutionOutcome::Limit {
                            limit,
                            receipt: nested,
                        } => {
                            receipt.state_visits += nested.state_visits;
                            receipt.transitions += nested.transitions;
                            receipt.capture_history += nested.capture_history;
                            return limited(limit, receipt);
                        }
                        _ => {}
                    }
                }
            }
        }
        receipt.transitions += 1;
        if receipt.transitions >= limits.max_steps {
            return limited(ExecutionLimit::Transitions, receipt);
        }
    }
    ExecutionOutcome::NoMatch { receipt }
}

fn push_at(stack: &mut Vec<(usize, Thread)>, position: usize, mut thread: Thread, state: StateId) {
    thread.state = state;
    stack.push((position, thread));
}

fn limited(limit: ExecutionLimit, receipt: ExecutionReceipt) -> ExecutionOutcome {
    ExecutionOutcome::Limit { limit, receipt }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ByteDomain, EnginePolicy, IrNode, PatternIr, RepeatBounds, compile};

    fn run(root: IrNode<u8, ()>, subject: &[u8], limits: TextLimits) -> ExecutionOutcome {
        let ir = PatternIr::<ByteDomain, ()>::new(root, BTreeMap::new(), &EnginePolicy::new([]))
            .unwrap();
        execute_regular(&compile(&ir), subject, limits, |_, _| false)
    }

    #[test]
    fn nested_ambiguous_repetition_has_linear_accounted_work() {
        let repeated_a = IrNode::Repeat {
            node: Box::new(IrNode::Alternation(vec![
                IrNode::Symbol(b'a'),
                IrNode::Concat(vec![IrNode::Symbol(b'a')]),
            ])),
            bounds: RepeatBounds::new(0, None).unwrap(),
            greedy: true,
        };
        let pattern = IrNode::Concat(vec![repeated_a, IrNode::Symbol(b'b')]);
        for length in [32, 128, 512] {
            let outcome = run(pattern.clone(), &vec![b'a'; length], TextLimits::default());
            let ExecutionOutcome::NoMatch { receipt } = outcome else {
                panic!("adversarial rejection must complete normally: {outcome:?}");
            };
            assert!(receipt.state_visits <= (length + 1) * receipt.state_count * 2);
        }
    }

    #[test]
    fn long_rejection_terminates_and_limits_are_typed() {
        let pattern = IrNode::Concat(vec![IrNode::Any, IrNode::Symbol(b'z')]);
        let subject = vec![b'a'; 10_000];
        assert!(matches!(
            run(pattern.clone(), &subject, TextLimits::default()),
            ExecutionOutcome::NoMatch { .. }
        ));
        let limits = TextLimits {
            max_steps: 1,
            ..TextLimits::default()
        };
        assert!(matches!(
            run(pattern, b"az", limits),
            ExecutionOutcome::Limit {
                limit: ExecutionLimit::Transitions,
                ..
            }
        ));
    }

    #[test]
    fn fixed_width_assertion_runs_without_consuming_subject() {
        let assertion = crate::AssertionId(7);
        let ir = PatternIr::<ByteDomain, ()>::new(
            IrNode::Concat(vec![IrNode::Assertion(assertion), IrNode::Symbol(b'a')]),
            BTreeMap::from([(assertion, IrNode::Symbol(b'a'))]),
            &EnginePolicy::new([]),
        )
        .unwrap();
        let outcome = execute_regular(&compile(&ir), b"a", TextLimits::default(), |_, _| false);
        assert!(matches!(
            outcome,
            ExecutionOutcome::Match {
                matched: ExecutionMatch { end: 1, .. },
                ..
            }
        ));
    }

    #[test]
    fn regular_pattern_keeps_the_pre_extension_receipt() {
        let outcome = run(IrNode::Symbol(b'a'), b"a", TextLimits::default());
        assert_eq!(
            outcome,
            ExecutionOutcome::Match {
                matched: ExecutionMatch {
                    start: 0,
                    end: 1,
                    captures: BTreeMap::new(),
                },
                receipt: ExecutionReceipt {
                    state_count: 2,
                    state_visits: 2,
                    transitions: 1,
                    capture_history: 0,
                    subject_symbols: 1,
                },
            }
        );
    }

    #[test]
    fn variable_width_assertion_is_a_typed_refusal() {
        let assertion = crate::AssertionId(9);
        let ir = PatternIr::<ByteDomain, ()>::new(
            IrNode::Assertion(assertion),
            BTreeMap::from([(
                assertion,
                IrNode::Repeat {
                    node: Box::new(IrNode::Symbol(b'a')),
                    bounds: RepeatBounds::new(0, None).unwrap(),
                    greedy: true,
                },
            )]),
            &EnginePolicy::new([]),
        )
        .unwrap();
        assert!(matches!(
            execute_regular(&compile(&ir), b"aaa", TextLimits::default(), |_, _| false),
            ExecutionOutcome::Unsupported {
                feature: UnsupportedFeature::VariableWidthAssertion(found),
                ..
            } if found == assertion
        ));
    }
}
