//! Legacy text-program compatibility lowering into the shared pattern engine.

use crate::{
    Anchor, CaptureId, EnginePolicy, ExecutionLimit, ExecutionOutcome, ExecutionReceipt, IrNode,
    PatternIr, RepeatBounds, ScalarDomain, UnsupportedFeature, compile, execute::search_spanning,
};
use std::collections::BTreeMap;

/// Character class understood by the shared text-pattern VM.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextClass {
    /// ASCII alphabetic characters.
    Alpha,
    /// ASCII digits.
    Digit,
    /// ASCII lowercase alphabetic characters.
    Lower,
    /// ASCII uppercase alphabetic characters.
    Upper,
    /// ASCII alphanumeric characters.
    Alnum,
    /// ASCII whitespace characters.
    Space,
    /// ASCII punctuation characters.
    Punct,
    /// ASCII hexadecimal digits.
    Hex,
    /// The NUL character.
    Zero,
    /// A literal/range set, optionally including nested classes.
    Set {
        /// Literal characters accepted by the set.
        chars: Vec<char>,
        /// Inclusive character ranges accepted by the set.
        ranges: Vec<(char, char)>,
        /// Nested reusable classes accepted by the set.
        classes: Vec<TextClass>,
        /// Inverts the accepted membership.
        negated: bool,
    },
    /// Inverts another class.
    Not(Box<TextClass>),
}

impl TextClass {
    /// Returns true when `ch` belongs to this class.
    pub fn matches(&self, ch: char) -> bool {
        match self {
            Self::Alpha => ch.is_ascii_alphabetic(),
            Self::Digit => ch.is_ascii_digit(),
            Self::Lower => ch.is_ascii_lowercase(),
            Self::Upper => ch.is_ascii_uppercase(),
            Self::Alnum => ch.is_ascii_alphanumeric(),
            Self::Space => ch.is_ascii_whitespace(),
            Self::Punct => ch.is_ascii_punctuation(),
            Self::Hex => ch.is_ascii_hexdigit(),
            Self::Zero => ch == '\0',
            Self::Set {
                chars,
                ranges,
                classes,
                negated,
            } => {
                let found = chars.contains(&ch)
                    || ranges.iter().any(|(start, end)| *start <= ch && ch <= *end)
                    || classes.iter().any(|class| class.matches(ch));
                if *negated { !found } else { found }
            }
            Self::Not(class) => !class.matches(ch),
        }
    }
}

/// One operation in the shared text-pattern VM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextOp {
    /// Match one character from a class.
    Class(TextClass),
    /// Match one literal character.
    Literal(char),
    /// Match any one character.
    Any,
    /// Start a capture at the current byte offset.
    CaptureStart,
    /// End the most recent open capture at the current byte offset.
    CaptureEnd,
    /// Quantify the previous consuming operation.
    Repeat {
        /// Minimum number of repetitions.
        min: usize,
        /// Maximum number of repetitions, or unbounded when absent.
        max: Option<usize>,
        /// Prefer longer repetitions before shorter ones.
        greedy: bool,
    },
    /// Match balanced text beginning with `open` and ending at its paired `close`.
    Balanced {
        /// Opening delimiter.
        open: char,
        /// Closing delimiter.
        close: char,
    },
    /// Match a frontier before a character in the class.
    Frontier(TextClass),
    /// Match the start of the subject.
    AnchorStart,
    /// Match the end of the subject.
    AnchorEnd,
}

/// A successful text-pattern match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMatch {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset.
    pub end: usize,
    /// Captured byte ranges.
    pub captures: Vec<(usize, usize)>,
}

/// Step limits for the bounded VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLimits {
    /// Maximum total transitions (the legacy text VM treats these as steps).
    pub max_steps: usize,
    /// Maximum automaton states admitted by one execution.
    pub max_states: usize,
    /// Maximum capture-boundary records retained by one execution.
    pub max_capture_history: usize,
    /// Maximum subject symbols inspected by one execution.
    pub max_subject_symbols: usize,
}

impl Default for TextLimits {
    fn default() -> Self {
        Self {
            max_steps: 10_000,
            max_states: 4_096,
            max_capture_history: 10_000,
            max_subject_symbols: 1_000_000,
        }
    }
}

#[derive(Clone, Debug)]
struct CursorText {
    chars: Vec<char>,
    offsets: Vec<usize>,
    len_bytes: usize,
}

impl CursorText {
    fn new(subject: &str) -> Self {
        let mut chars = Vec::new();
        let mut offsets = Vec::new();
        for (offset, ch) in subject.char_indices() {
            offsets.push(offset);
            chars.push(ch);
        }
        Self {
            chars,
            offsets,
            len_bytes: subject.len(),
        }
    }

    fn cursor_for_byte(&self, byte: usize) -> Option<usize> {
        if byte == self.len_bytes {
            return Some(self.chars.len());
        }
        self.offsets.iter().position(|offset| *offset == byte)
    }

    fn byte_for_cursor(&self, cursor: usize) -> usize {
        self.offsets.get(cursor).copied().unwrap_or(self.len_bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TextExtension {
    Class(TextClass),
    Balanced { open: char, close: char },
    Frontier(TextClass),
}

/// Typed outcome of one globally bounded pattern search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternSearchOutcome<M> {
    /// The leftmost admitted match.
    Match {
        /// Match value in the adapter's coordinate domain.
        matched: M,
        /// Work consumed by the complete search.
        receipt: ExecutionReceipt,
    },
    /// Definitive rejection after examining every admitted start.
    NoMatch {
        /// Work consumed by the complete search.
        receipt: ExecutionReceipt,
    },
    /// A configured resource boundary stopped the search.
    Limit {
        /// Exhausted resource.
        limit: ExecutionLimit,
        /// Work consumed before stopping.
        receipt: ExecutionReceipt,
    },
    /// A construct outside the regular engine was encountered.
    Unsupported {
        /// Exact unsupported construct.
        feature: UnsupportedFeature,
        /// Work consumed before discovering it.
        receipt: ExecutionReceipt,
    },
}

impl<M> PatternSearchOutcome<M> {
    /// Returns the match while deliberately discarding the receipt.
    pub fn into_match(self) -> Option<M> {
        match self {
            Self::Match { matched, .. } => Some(matched),
            _ => None,
        }
    }

    /// Reports whether this outcome contains a match.
    pub fn is_some(&self) -> bool {
        matches!(self, Self::Match { .. })
    }

    /// Reports whether this outcome is a definitive no-match.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoMatch { .. })
    }

    /// Returns the match, panicking when the outcome is not a match.
    pub fn unwrap(self) -> M {
        self.into_match()
            .expect("pattern search outcome was not a match")
    }

    /// Applies a function to a match and deliberately discards other outcomes.
    pub fn map<T>(self, f: impl FnOnce(M) -> T) -> Option<T> {
        self.into_match().map(f)
    }
}

/// Runs a compiled text pattern over `subject` starting at byte offset `init`.
///
/// Unanchored programs search forward from `init`; programs beginning with
/// [`TextOp::AnchorStart`] only attempt a match at subject start. The matcher
/// fails closed when `limits.max_steps` is reached.
pub fn run_text_pattern(
    ops: &[TextOp],
    subject: &str,
    init: usize,
    limits: TextLimits,
) -> PatternSearchOutcome<TextMatch> {
    let Some(ir) = lower_text_program(ops) else {
        return PatternSearchOutcome::NoMatch {
            receipt: ExecutionReceipt::default(),
        };
    };
    let automaton = compile(&ir);
    let text = CursorText::new(subject);
    let Some(init_cursor) = text.cursor_for_byte(init) else {
        return PatternSearchOutcome::NoMatch {
            receipt: ExecutionReceipt::default(),
        };
    };
    let outcome = search_spanning(
        &automaton,
        &text.chars,
        init_cursor,
        limits,
        |extension, subject, position| match extension {
            TextExtension::Class(class) => subject
                .get(position)
                .is_some_and(|ch| class.matches(*ch))
                .then_some(position + 1),
            TextExtension::Balanced { open, close } => {
                match_balanced(subject, position, *open, *close)
            }
            TextExtension::Frontier(class) => {
                let previous = position.checked_sub(1).and_then(|i| subject.get(i));
                let current = subject.get(position);
                (!previous.is_some_and(|ch| class.matches(*ch))
                    && current.is_some_and(|ch| class.matches(*ch)))
                .then_some(position)
            }
        },
    );
    match outcome {
        ExecutionOutcome::Match { matched, receipt } => {
            let captures = matched
                .captures
                .values()
                .map(|span| {
                    (
                        text.byte_for_cursor(span.start),
                        text.byte_for_cursor(span.end),
                    )
                })
                .collect();
            PatternSearchOutcome::Match {
                matched: TextMatch {
                    start: text.byte_for_cursor(matched.start),
                    end: text.byte_for_cursor(matched.end),
                    captures,
                },
                receipt,
            }
        }
        ExecutionOutcome::NoMatch { receipt } => PatternSearchOutcome::NoMatch { receipt },
        ExecutionOutcome::Limit { limit, receipt } => {
            PatternSearchOutcome::Limit { limit, receipt }
        }
        ExecutionOutcome::Unsupported { feature, receipt } => {
            PatternSearchOutcome::Unsupported { feature, receipt }
        }
    }
}

fn lower_text_program(ops: &[TextOp]) -> Option<PatternIr<ScalarDomain, TextExtension>> {
    let mut frames = vec![Vec::new()];
    let mut next_capture = 0u32;
    for op in ops {
        let nodes = frames.last_mut()?;
        match op {
            TextOp::Class(class) => {
                nodes.push(IrNode::Extension(TextExtension::Class(class.clone())))
            }
            TextOp::Literal(ch) => nodes.push(IrNode::Symbol(*ch)),
            TextOp::Any => nodes.push(IrNode::Any),
            TextOp::Balanced { open, close } => {
                nodes.push(IrNode::Extension(TextExtension::Balanced {
                    open: *open,
                    close: *close,
                }))
            }
            TextOp::Repeat { min, max, greedy } => {
                let node = nodes.pop()?;
                nodes.push(IrNode::Repeat {
                    node: Box::new(node),
                    bounds: RepeatBounds::new(*min, *max).ok()?,
                    greedy: *greedy,
                });
            }
            TextOp::CaptureStart => frames.push(Vec::new()),
            TextOp::CaptureEnd => {
                if frames.len() == 1 {
                    return None;
                }
                let body = IrNode::Concat(frames.pop()?);
                let id = CaptureId(next_capture);
                next_capture += 1;
                frames.last_mut()?.push(IrNode::Capture {
                    id,
                    node: Box::new(body),
                });
            }
            TextOp::Frontier(class) => {
                nodes.push(IrNode::Extension(TextExtension::Frontier(class.clone())))
            }
            TextOp::AnchorStart => nodes.push(IrNode::Anchor(Anchor::SubjectStart)),
            TextOp::AnchorEnd => nodes.push(IrNode::Anchor(Anchor::SubjectEnd)),
        }
    }
    if frames.len() != 1 {
        return None;
    }
    let extensions = ops.iter().filter_map(|op| match op {
        TextOp::Class(class) => Some(TextExtension::Class(class.clone())),
        TextOp::Balanced { open, close } => Some(TextExtension::Balanced {
            open: *open,
            close: *close,
        }),
        TextOp::Frontier(class) => Some(TextExtension::Frontier(class.clone())),
        _ => None,
    });
    PatternIr::new(
        IrNode::Concat(frames.pop()?),
        BTreeMap::new(),
        &EnginePolicy::new(extensions),
    )
    .ok()
}

fn match_balanced(text: &[char], cursor: usize, open: char, close: char) -> Option<usize> {
    if text.get(cursor).copied() != Some(open) {
        return None;
    }
    let mut depth = 0usize;
    for (index, ch) in text.iter().copied().enumerate().skip(cursor) {
        if ch == open {
            depth += 1;
        }
        if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index + 1);
            }
        }
    }
    None
}
