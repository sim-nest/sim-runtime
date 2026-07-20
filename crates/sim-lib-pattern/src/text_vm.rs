//! Bounded text-pattern virtual machine shared by pattern dialects.

/// Character class understood by the shared text-pattern VM.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Maximum recursive VM steps before the match fails closed.
    pub max_steps: usize,
}

impl Default for TextLimits {
    fn default() -> Self {
        Self { max_steps: 10_000 }
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Atom {
    Class(TextClass),
    Literal(char),
    Any,
    Balanced { open: char, close: char },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Quantifier {
    min: usize,
    max: Option<usize>,
    greedy: bool,
}

impl Default for Quantifier {
    fn default() -> Self {
        Self {
            min: 1,
            max: Some(1),
            greedy: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Unit {
    Atom(Atom, Quantifier),
    CaptureStart,
    CaptureEnd,
    Frontier(TextClass),
    AnchorStart,
    AnchorEnd,
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
) -> Option<TextMatch> {
    let units = compile_units(ops)?;
    let text = CursorText::new(subject);
    let init_cursor = text.cursor_for_byte(init)?;
    let anchored = matches!(units.first(), Some(Unit::AnchorStart));
    let starts: Box<dyn Iterator<Item = usize>> = if anchored {
        Box::new(std::iter::once(init_cursor).filter(|cursor| *cursor == 0))
    } else {
        Box::new(init_cursor..=text.chars.len())
    };

    for start_cursor in starts {
        let mut engine = MatchEngine::new(&units, &text, limits.max_steps);
        if let Some((end_cursor, captures)) =
            engine.match_from(0, start_cursor, Vec::new(), Vec::new())
        {
            return Some(TextMatch {
                start: text.byte_for_cursor(start_cursor),
                end: text.byte_for_cursor(end_cursor),
                captures,
            });
        }
    }
    None
}

fn compile_units(ops: &[TextOp]) -> Option<Vec<Unit>> {
    let mut units = Vec::new();
    for op in ops {
        match op {
            TextOp::Class(class) => units.push(Unit::Atom(
                Atom::Class(class.clone()),
                Quantifier::default(),
            )),
            TextOp::Literal(ch) => {
                units.push(Unit::Atom(Atom::Literal(*ch), Quantifier::default()))
            }
            TextOp::Any => units.push(Unit::Atom(Atom::Any, Quantifier::default())),
            TextOp::Balanced { open, close } => units.push(Unit::Atom(
                Atom::Balanced {
                    open: *open,
                    close: *close,
                },
                Quantifier::default(),
            )),
            TextOp::Repeat { min, max, greedy } => {
                let Some(Unit::Atom(_, quantifier)) = units.last_mut() else {
                    return None;
                };
                *quantifier = Quantifier {
                    min: *min,
                    max: *max,
                    greedy: *greedy,
                };
            }
            TextOp::CaptureStart => units.push(Unit::CaptureStart),
            TextOp::CaptureEnd => units.push(Unit::CaptureEnd),
            TextOp::Frontier(class) => units.push(Unit::Frontier(class.clone())),
            TextOp::AnchorStart => units.push(Unit::AnchorStart),
            TextOp::AnchorEnd => units.push(Unit::AnchorEnd),
        }
    }
    Some(units)
}

struct MatchEngine<'a> {
    units: &'a [Unit],
    text: &'a CursorText,
    limit: usize,
    steps: usize,
}

impl<'a> MatchEngine<'a> {
    fn new(units: &'a [Unit], text: &'a CursorText, limit: usize) -> Self {
        Self {
            units,
            text,
            limit,
            steps: 0,
        }
    }

    fn match_from(
        &mut self,
        unit_index: usize,
        cursor: usize,
        captures: Vec<(usize, usize)>,
        open_captures: Vec<usize>,
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        self.steps += 1;
        if self.steps > self.limit {
            return None;
        }
        let Some(unit) = self.units.get(unit_index) else {
            return if open_captures.is_empty() {
                Some((cursor, captures))
            } else {
                None
            };
        };
        match unit {
            Unit::Atom(atom, quantifier) => {
                let positions = repeated_positions(atom, *quantifier, self.text, cursor);
                for next_cursor in positions {
                    if let Some(result) = self.match_from(
                        unit_index + 1,
                        next_cursor,
                        captures.clone(),
                        open_captures.clone(),
                    ) {
                        return Some(result);
                    }
                }
                None
            }
            Unit::CaptureStart => {
                let mut open = open_captures;
                open.push(self.text.byte_for_cursor(cursor));
                self.match_from(unit_index + 1, cursor, captures, open)
            }
            Unit::CaptureEnd => {
                let mut open = open_captures;
                let start = open.pop()?;
                let mut captures = captures;
                captures.push((start, self.text.byte_for_cursor(cursor)));
                self.match_from(unit_index + 1, cursor, captures, open)
            }
            Unit::Frontier(class) => {
                let previous = cursor
                    .checked_sub(1)
                    .and_then(|index| self.text.chars.get(index));
                let current = self.text.chars.get(cursor);
                let previous_matches = previous.is_some_and(|ch| class.matches(*ch));
                let current_matches = current.is_some_and(|ch| class.matches(*ch));
                if !previous_matches && current_matches {
                    self.match_from(unit_index + 1, cursor, captures, open_captures)
                } else {
                    None
                }
            }
            Unit::AnchorStart => {
                if cursor == 0 {
                    self.match_from(unit_index + 1, cursor, captures, open_captures)
                } else {
                    None
                }
            }
            Unit::AnchorEnd => {
                if cursor == self.text.chars.len() {
                    self.match_from(unit_index + 1, cursor, captures, open_captures)
                } else {
                    None
                }
            }
        }
    }
}

fn repeated_positions(
    atom: &Atom,
    quantifier: Quantifier,
    text: &CursorText,
    cursor: usize,
) -> Vec<usize> {
    let mut positions = vec![cursor];
    let max = quantifier
        .max
        .unwrap_or_else(|| text.chars.len().saturating_sub(cursor));
    let mut current = cursor;
    for _ in 0..max {
        let Some(next) = match_atom(atom, text, current) else {
            break;
        };
        if next == current {
            break;
        }
        positions.push(next);
        current = next;
    }
    let mut selected = positions
        .into_iter()
        .enumerate()
        .filter_map(|(count, position)| (count >= quantifier.min).then_some(position))
        .collect::<Vec<_>>();
    if quantifier.greedy {
        selected.reverse();
    }
    selected
}

fn match_atom(atom: &Atom, text: &CursorText, cursor: usize) -> Option<usize> {
    match atom {
        Atom::Class(class) => text
            .chars
            .get(cursor)
            .is_some_and(|ch| class.matches(*ch))
            .then_some(cursor + 1),
        Atom::Literal(expected) => text
            .chars
            .get(cursor)
            .is_some_and(|ch| ch == expected)
            .then_some(cursor + 1),
        Atom::Any => (cursor < text.chars.len()).then_some(cursor + 1),
        Atom::Balanced { open, close } => match_balanced(text, cursor, *open, *close),
    }
}

fn match_balanced(text: &CursorText, cursor: usize, open: char, close: char) -> Option<usize> {
    if text.chars.get(cursor).copied() != Some(open) {
        return None;
    }
    let mut depth = 0usize;
    for index in cursor..text.chars.len() {
        let ch = text.chars[index];
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
