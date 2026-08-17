//! Explicitly opt-in, separately metered execution for non-regular constructs.

use crate::{AssertionId, CaptureId};
use std::collections::VecDeque;

/// Stable kinds understood by the non-regular extension lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionKind {
    /// Compare input with an earlier capture.
    Backreference(CaptureId),
    /// Evaluate an assertion whose width cannot be established statically.
    VariableWidthAssertion(AssertionId),
}

/// Independent limits for work that can invalidate regular worst-case bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionLimits {
    /// Maximum capture bytes or code units inspected.
    pub max_capture_units: usize,
    /// Maximum queue entries evaluated.
    pub max_work_items: usize,
}

/// Exact non-regular work charged by an attempt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtensionReceipt {
    /// Capture bytes or code units inspected.
    pub capture_units: usize,
    /// Queue entries evaluated.
    pub work_items: usize,
}

/// Typed reason that extension execution did not produce a match decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionRefusal {
    /// No installed extension admitted this operation.
    Unsupported(ExtensionKind),
    /// The capture-byte or code-unit allowance was exhausted.
    CaptureUnits,
    /// The recursion-free work queue allowance was exhausted.
    WorkItems,
}

/// One unit placed onto the bounded extension queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionWork<T> {
    /// Extension-private, caller-defined work payload.
    pub payload: T,
    /// Capture bytes or code units this item will inspect.
    pub capture_units: usize,
}

/// Result of a bounded non-regular attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionOutcome {
    /// The extension accepted.
    Match(ExtensionReceipt),
    /// The extension rejected conclusively.
    NoMatch(ExtensionReceipt),
    /// The feature was unsupported or its independent budget was exhausted.
    Refused {
        /// Exact refusal.
        reason: ExtensionRefusal,
        /// Work consumed before refusal.
        receipt: ExtensionReceipt,
    },
}

/// Result of evaluating one queue item without recursive calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionStep<T> {
    /// This branch matched.
    Match,
    /// This branch ended without matching.
    NoMatch,
    /// Add bounded continuation work to the queue.
    Continue(Vec<ExtensionWork<T>>),
}

/// Opt-in implementation of one non-regular feature family.
pub trait BoundedExtension {
    /// Extension-private queue payload.
    type Work;

    /// Reports whether this implementation admits `kind` and creates initial work.
    fn start(&self, kind: ExtensionKind) -> Option<Vec<ExtensionWork<Self::Work>>>;

    /// Evaluates exactly one item. Implementations continue through returned work,
    /// never by recursively invoking the executor.
    fn step(&self, work: Self::Work) -> ExtensionStep<Self::Work>;
}

/// Executes an admitted non-regular operation using an independent FIFO budget.
pub fn execute_extension<X: BoundedExtension>(
    extension: &X,
    kind: ExtensionKind,
    limits: ExtensionLimits,
) -> ExtensionOutcome {
    let Some(initial) = extension.start(kind) else {
        return ExtensionOutcome::Refused {
            reason: ExtensionRefusal::Unsupported(kind),
            receipt: ExtensionReceipt::default(),
        };
    };
    let mut queue = VecDeque::from(initial);
    let mut receipt = ExtensionReceipt::default();
    while let Some(work) = queue.pop_front() {
        if receipt.work_items == limits.max_work_items {
            return refused(ExtensionRefusal::WorkItems, receipt);
        }
        if work.capture_units
            > limits
                .max_capture_units
                .saturating_sub(receipt.capture_units)
        {
            return refused(ExtensionRefusal::CaptureUnits, receipt);
        }
        receipt.work_items += 1;
        receipt.capture_units += work.capture_units;
        match extension.step(work.payload) {
            ExtensionStep::Match => return ExtensionOutcome::Match(receipt),
            ExtensionStep::NoMatch => {}
            ExtensionStep::Continue(next) => queue.extend(next),
        }
    }
    ExtensionOutcome::NoMatch(receipt)
}

fn refused(reason: ExtensionRefusal, receipt: ExtensionReceipt) -> ExtensionOutcome {
    ExtensionOutcome::Refused { reason, receipt }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Backreference;

    impl BoundedExtension for Backreference {
        type Work = usize;

        fn start(&self, kind: ExtensionKind) -> Option<Vec<ExtensionWork<Self::Work>>> {
            matches!(kind, ExtensionKind::Backreference(_)).then(|| {
                vec![ExtensionWork {
                    payload: 0,
                    capture_units: 3,
                }]
            })
        }

        fn step(&self, offset: usize) -> ExtensionStep<Self::Work> {
            if offset == 3 {
                ExtensionStep::Match
            } else {
                ExtensionStep::Continue(vec![ExtensionWork {
                    payload: offset + 1,
                    capture_units: 3,
                }])
            }
        }
    }

    #[test]
    fn backreference_exhausts_capture_budget_deterministically() {
        let limits = ExtensionLimits {
            max_capture_units: 6,
            max_work_items: 8,
        };
        let expected = ExtensionOutcome::Refused {
            reason: ExtensionRefusal::CaptureUnits,
            receipt: ExtensionReceipt {
                capture_units: 6,
                work_items: 2,
            },
        };
        for _ in 0..3 {
            assert_eq!(
                execute_extension(
                    &Backreference,
                    ExtensionKind::Backreference(CaptureId(1)),
                    limits
                ),
                expected
            );
        }
    }

    #[test]
    fn unsupported_extensions_are_typed() {
        assert!(matches!(
            execute_extension(
                &Backreference,
                ExtensionKind::VariableWidthAssertion(AssertionId(4)),
                ExtensionLimits {
                    max_capture_units: 10,
                    max_work_items: 10
                }
            ),
            ExtensionOutcome::Refused {
                reason: ExtensionRefusal::Unsupported(ExtensionKind::VariableWidthAssertion(
                    AssertionId(4)
                )),
                ..
            }
        ));
    }
}
