//! Typed adapters from concrete text domains to the regular pattern executor.

use crate::{
    Automaton, ByteDomain, ByteOffset, CaptureId, CodeUnitDomain, CodeUnitOffset, ExecutionLimit,
    ExecutionOutcome, ExecutionReceipt, ScalarDomain, ScalarOffset, SymbolDomain, TextLimits,
    UnsupportedFeature, execute_regular,
};
use sim_text::CodeUnitString;
use std::collections::BTreeMap;

/// A half-open capture span whose offset type identifies its subject domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainCaptureSpan<D: SymbolDomain> {
    /// Inclusive start offset.
    pub start: D::Offset,
    /// Exclusive end offset.
    pub end: D::Offset,
}

/// A successful match whose offsets cannot be mixed with another domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainMatch<D: SymbolDomain> {
    /// Inclusive start offset.
    pub start: D::Offset,
    /// Exclusive end offset.
    pub end: D::Offset,
    /// Captures keyed by their stable compiled identifier.
    pub captures: BTreeMap<CaptureId, DomainCaptureSpan<D>>,
}

/// A resource-accounted execution result with domain-typed match positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainExecutionOutcome<D: SymbolDomain> {
    /// The automaton accepted a subject prefix.
    Match {
        /// Match and captures in the selected offset domain.
        matched: DomainMatch<D>,
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
    /// The construct belongs to the separately budgeted extension lane.
    Unsupported {
        /// Exact unsupported construct.
        feature: UnsupportedFeature,
        /// Regular work consumed before discovering it.
        receipt: ExecutionReceipt,
    },
}

trait IndexedDomain: SymbolDomain {
    fn offset(index: usize) -> Self::Offset;
}

impl IndexedDomain for ByteDomain {
    fn offset(index: usize) -> Self::Offset {
        ByteOffset(index)
    }
}

impl IndexedDomain for ScalarDomain {
    fn offset(index: usize) -> Self::Offset {
        ScalarOffset::new(index)
    }
}

impl IndexedDomain for CodeUnitDomain {
    fn offset(index: usize) -> Self::Offset {
        CodeUnitOffset::new(index)
    }
}

fn typed<D: IndexedDomain>(outcome: ExecutionOutcome) -> DomainExecutionOutcome<D> {
    match outcome {
        ExecutionOutcome::Match { matched, receipt } => DomainExecutionOutcome::Match {
            matched: DomainMatch {
                start: D::offset(matched.start),
                end: D::offset(matched.end),
                captures: matched
                    .captures
                    .into_iter()
                    .map(|(id, span)| {
                        (
                            id,
                            DomainCaptureSpan {
                                start: D::offset(span.start),
                                end: D::offset(span.end),
                            },
                        )
                    })
                    .collect(),
            },
            receipt,
        },
        ExecutionOutcome::NoMatch { receipt } => DomainExecutionOutcome::NoMatch { receipt },
        ExecutionOutcome::Limit { limit, receipt } => {
            DomainExecutionOutcome::Limit { limit, receipt }
        }
        ExecutionOutcome::Unsupported { feature, receipt } => {
            DomainExecutionOutcome::Unsupported { feature, receipt }
        }
    }
}

/// Execute a byte-domain automaton over exact bytes.
pub fn execute_bytes<E>(
    automaton: &Automaton<u8, E>,
    subject: &[u8],
    limits: TextLimits,
    extension_matches: impl Fn(&E, &u8) -> bool,
) -> DomainExecutionOutcome<ByteDomain> {
    typed(execute_regular(
        automaton,
        subject,
        limits,
        extension_matches,
    ))
}

/// Execute a scalar-domain automaton over Unicode scalar values.
pub fn execute_scalars<E>(
    automaton: &Automaton<char, E>,
    subject: &[char],
    limits: TextLimits,
    extension_matches: impl Fn(&E, &char) -> bool,
) -> DomainExecutionOutcome<ScalarDomain> {
    typed(execute_regular(
        automaton,
        subject,
        limits,
        extension_matches,
    ))
}

/// Execute a code-unit-domain automaton over an exact `sim-text` value.
///
/// Unlike scalar execution, every position between adjacent `u16` values is
/// addressable, including the middle of a surrogate pair and either side of a
/// lone surrogate.
///
/// ```compile_fail
/// use sim_lib_pattern::{CodeUnitOffset, ScalarOffset, require_code_unit_offset};
/// require_code_unit_offset(ScalarOffset::new(1));
/// ```
pub fn execute_code_units<E>(
    automaton: &Automaton<u16, E>,
    subject: &CodeUnitString,
    limits: TextLimits,
    extension_matches: impl Fn(&E, &u16) -> bool,
) -> DomainExecutionOutcome<CodeUnitDomain> {
    typed(execute_regular(
        automaton,
        subject.as_code_units(),
        limits,
        extension_matches,
    ))
}

/// Type-checking witness used by APIs that require an exact code-unit offset.
pub const fn require_code_unit_offset(offset: CodeUnitOffset) -> CodeUnitOffset {
    offset
}
