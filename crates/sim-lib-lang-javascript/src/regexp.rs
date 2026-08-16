// conformance: ECMAScript regular expressions lower into the shared pattern organ.

//! Faithful, deliberately narrow ECMAScript RegExp compiler.

use sim_lib_pattern::{
    Anchor, Automaton, CaptureId, CodeUnitDomain, DomainExecutionOutcome, EnginePolicy, IrNode,
    PatternIr, RepeatBounds, TextLimits, TextMatch, compile, execute_code_units,
};
use sim_text::CodeUnitString;
use std::collections::{BTreeMap, BTreeSet};

/// Required successor for usable ECMAScript regular expressions.
pub const JAVASCRIPT_REGEXP_SUCCESSOR: &str = "remaining ECMAScript RegExp clauses";

/// An explicitly unsupported ECMAScript RegExp clause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptRegExpGap {
    /// Any flag (`d g i m s u v y`) is currently rejected; no stateful `lastIndex` is emulated.
    Flags,
    /// Backreferences.
    Backreferences,
    /// Lookahead or lookbehind.
    Lookaround,
    /// Unicode property escapes and Unicode-set notation.
    UnicodeProperties,
    /// Word-boundary assertions.
    WordBoundary,
}
/// Exact first-release gap manifest.
pub const fn javascript_regexp_gaps() -> &'static [JavascriptRegExpGap] {
    &[
        JavascriptRegExpGap::Flags,
        JavascriptRegExpGap::Backreferences,
        JavascriptRegExpGap::Lookaround,
        JavascriptRegExpGap::UnicodeProperties,
        JavascriptRegExpGap::WordBoundary,
    ]
}
/// RegExp admission error; unsupported syntax is never approximated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptRegExpError {
    /// A flag is unsupported.
    UnsupportedFlag(char),
    /// Syntax is unsupported or malformed at a byte offset.
    UnsupportedSyntax {
        /// Byte offset.
        offset: usize,
        /// Stable explanation.
        reason: &'static str,
    },
}
/// A faithfully compiled RegExp program for the shared bounded text VM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavascriptRegExp {
    source: String,
    automaton: Automaton<u16, CodeUnitClass>,
    anchored_start: bool,
}
impl JavascriptRegExp {
    /// Compile the regular intersection, including alternation, groups,
    /// captures, anchors, classes, and greedy/lazy counted quantifiers.
    pub fn compile(source: &str, flags: &str) -> Result<Self, JavascriptRegExpError> {
        if let Some(flag) = flags.chars().next() {
            return Err(JavascriptRegExpError::UnsupportedFlag(flag));
        }
        let root = Parser::new(source).parse()?;
        let policy = EnginePolicy::new(classes_in(&root));
        let ir = PatternIr::<CodeUnitDomain, CodeUnitClass>::new(root, BTreeMap::new(), &policy)
            .map_err(|_| syntax(0, "invalid regular expression"))?;
        Ok(Self {
            source: source.into(),
            automaton: compile(&ir),
            anchored_start: source.starts_with('^'),
        })
    }
    /// Original source.
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Execute under an explicit shared-engine transition bound.
    /// Match and capture offsets are ECMAScript UTF-16 code-unit offsets.
    pub fn find(&self, subject: &str, init: usize, max_steps: usize) -> Option<TextMatch> {
        let subject = CodeUnitString::from_scalar(subject);
        let mut starts = if self.anchored_start {
            (init == 0).then_some(0..=0)
        } else {
            Some(init..=subject.len())
        }?;
        starts.find_map(|start| {
            let tail = CodeUnitString::from_code_units(subject.as_code_units()[start..].to_vec());
            match execute_code_units(
                &self.automaton,
                &tail,
                TextLimits {
                    max_steps,
                    ..TextLimits::default()
                },
                |class, unit| class.matches(*unit),
            ) {
                DomainExecutionOutcome::Match { matched, .. } => Some(TextMatch {
                    start: start + matched.start.get(),
                    end: start + matched.end.get(),
                    captures: matched
                        .captures
                        .values()
                        .map(|span| (start + span.start.get(), start + span.end.get()))
                        .collect(),
                }),
                _ => None,
            }
        })
    }
}
fn syntax(offset: usize, reason: &'static str) -> JavascriptRegExpError {
    JavascriptRegExpError::UnsupportedSyntax { offset, reason }
}
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CodeUnitClass {
    Digit(bool),
    Space(bool),
    Word(bool),
    LineTerminator(bool),
    Set {
        units: Vec<u16>,
        ranges: Vec<(u16, u16)>,
        classes: Vec<CodeUnitClass>,
        negated: bool,
    },
}
impl CodeUnitClass {
    fn matches(&self, unit: u16) -> bool {
        match self {
            Self::Digit(negated) => (b'0' as u16..=b'9' as u16).contains(&unit) != *negated,
            Self::Space(negated) => {
                matches!(unit, 0x09..=0x0d | 0x20 | 0x00a0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
                    != *negated
            }
            Self::Word(negated) => {
                ((b'0' as u16..=b'9' as u16).contains(&unit)
                    || (b'A' as u16..=b'Z' as u16).contains(&unit)
                    || (b'a' as u16..=b'z' as u16).contains(&unit)
                    || unit == b'_' as u16)
                    != *negated
            }
            Self::LineTerminator(negated) => {
                matches!(unit, 0x0a | 0x0d | 0x2028 | 0x2029) != *negated
            }
            Self::Set {
                units,
                ranges,
                classes,
                negated,
            } => {
                (units.contains(&unit)
                    || ranges.iter().any(|(a, b)| *a <= unit && unit <= *b)
                    || classes.iter().any(|class| class.matches(unit)))
                    != *negated
            }
        }
    }
}
fn classes_in(root: &IrNode<u16, CodeUnitClass>) -> BTreeSet<CodeUnitClass> {
    fn visit(node: &IrNode<u16, CodeUnitClass>, found: &mut BTreeSet<CodeUnitClass>) {
        match node {
            IrNode::Extension(class) => {
                found.insert(class.clone());
            }
            IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
                nodes.iter().for_each(|node| visit(node, found))
            }
            IrNode::Repeat { node, .. } | IrNode::Group(node) | IrNode::Capture { node, .. } => {
                visit(node, found)
            }
            IrNode::Symbol(_) | IrNode::Any | IrNode::Anchor(_) | IrNode::Assertion(_) => {}
        }
    }
    let mut found = BTreeSet::new();
    visit(root, &mut found);
    found
}
fn escape_atom(
    ch: char,
    offset: usize,
) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
    Ok(match ch {
        'd' => IrNode::Extension(CodeUnitClass::Digit(false)),
        'D' => IrNode::Extension(CodeUnitClass::Digit(true)),
        's' => IrNode::Extension(CodeUnitClass::Space(false)),
        'S' => IrNode::Extension(CodeUnitClass::Space(true)),
        'w' => IrNode::Extension(CodeUnitClass::Word(false)),
        'W' => IrNode::Extension(CodeUnitClass::Word(true)),
        'b' | 'B' => return Err(syntax(offset, "word-boundary assertions are unsupported")),
        '1'..='9' => return Err(syntax(offset, "backreferences are unsupported")),
        'p' | 'P' => return Err(syntax(offset, "Unicode property escapes are unsupported")),
        other => literal(other),
    })
}
fn literal(ch: char) -> IrNode<u16, CodeUnitClass> {
    let nodes = ch
        .encode_utf16(&mut [0; 2])
        .iter()
        .copied()
        .map(IrNode::Symbol)
        .collect::<Vec<_>>();
    if nodes.len() == 1 {
        nodes.into_iter().next().unwrap()
    } else {
        IrNode::Concat(nodes)
    }
}

struct Parser {
    chars: Vec<(usize, char)>,
    at: usize,
    capture: u32,
}
impl Parser {
    fn new(source: &str) -> Self {
        Self {
            chars: source.char_indices().collect(),
            at: 0,
            capture: 0,
        }
    }
    fn parse(mut self) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let root = self.alternation()?;
        if let Some((offset, _)) = self.peek() {
            return Err(syntax(offset, "unmatched closing parenthesis"));
        }
        Ok(root)
    }
    fn alternation(&mut self) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let mut branches = vec![self.sequence()?];
        while self.take('|') {
            branches.push(self.sequence()?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap()
        } else {
            IrNode::Alternation(branches)
        })
    }
    fn sequence(&mut self) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let mut nodes = Vec::new();
        while self.peek().is_some_and(|(_, ch)| ch != ')' && ch != '|') {
            nodes.push(self.quantified()?);
        }
        Ok(IrNode::Concat(nodes))
    }
    fn quantified(&mut self) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let mut node = self.atom()?;
        let Some((offset, ch)) = self.peek() else {
            return Ok(node);
        };
        let bounds = match ch {
            '*' => {
                self.at += 1;
                Some(RepeatBounds::new(0, None).unwrap())
            }
            '+' => {
                self.at += 1;
                Some(RepeatBounds::new(1, None).unwrap())
            }
            '?' => {
                self.at += 1;
                Some(RepeatBounds::new(0, Some(1)).unwrap())
            }
            '{' => Some(self.counted(offset)?),
            _ => None,
        };
        if let Some(bounds) = bounds {
            let greedy = !self.take('?');
            node = IrNode::Repeat {
                node: Box::new(node),
                bounds,
                greedy,
            };
        }
        Ok(node)
    }
    fn counted(&mut self, offset: usize) -> Result<RepeatBounds, JavascriptRegExpError> {
        self.at += 1;
        let min = self
            .number()
            .ok_or_else(|| syntax(offset, "malformed counted quantifier"))?;
        let max = if self.take('}') {
            Some(min)
        } else if self.take(',') {
            let max = self.number();
            if !self.take('}') {
                return Err(syntax(offset, "unterminated counted quantifier"));
            }
            max
        } else {
            return Err(syntax(offset, "malformed counted quantifier"));
        };
        RepeatBounds::new(min, max)
            .map_err(|_| syntax(offset, "counted quantifier maximum is below minimum"))
    }
    fn number(&mut self) -> Option<usize> {
        let start = self.at;
        let mut value = 0usize;
        while let Some((_, ch)) = self.peek().filter(|(_, ch)| ch.is_ascii_digit()) {
            value = value
                .checked_mul(10)?
                .checked_add(ch.to_digit(10)? as usize)?;
            self.at += 1;
        }
        (self.at > start).then_some(value)
    }
    fn atom(&mut self) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let (offset, ch) = self.peek().ok_or_else(|| syntax(0, "missing atom"))?;
        self.at += 1;
        match ch {
            '^' => Ok(IrNode::Anchor(Anchor::SubjectStart)),
            '$' => Ok(IrNode::Anchor(Anchor::SubjectEnd)),
            '.' => Ok(IrNode::Extension(CodeUnitClass::LineTerminator(true))),
            '(' => {
                let capture = if self.take('?') {
                    if self.take(':') {
                        None
                    } else {
                        return Err(syntax(
                            offset,
                            "lookaround and special groups are unsupported",
                        ));
                    }
                } else {
                    let id = CaptureId(self.capture);
                    self.capture += 1;
                    Some(id)
                };
                let node = self.alternation()?;
                if !self.take(')') {
                    return Err(syntax(offset, "unterminated group"));
                }
                Ok(match capture {
                    Some(id) => IrNode::Capture {
                        id,
                        node: Box::new(node),
                    },
                    None => IrNode::Group(Box::new(node)),
                })
            }
            '[' => self.class(offset),
            '\\' => {
                let (_, escaped) = self
                    .peek()
                    .ok_or_else(|| syntax(offset, "trailing escape"))?;
                self.at += 1;
                escape_atom(escaped, offset)
            }
            '*' | '+' | '?' | '{' => Err(syntax(offset, "quantifier has no admissible atom")),
            ')' | '|' => unreachable!(),
            _ => Ok(literal(ch)),
        }
    }
    fn class(
        &mut self,
        offset: usize,
    ) -> Result<IrNode<u16, CodeUnitClass>, JavascriptRegExpError> {
        let negated = self.take('^');
        let mut units = Vec::new();
        let mut ranges = Vec::new();
        let mut classes = Vec::new();
        while let Some((pos, ch)) = self.peek() {
            if ch == ']' {
                self.at += 1;
                return Ok(IrNode::Extension(CodeUnitClass::Set {
                    units,
                    ranges,
                    classes,
                    negated,
                }));
            }
            self.at += 1;
            let first = if ch == '\\' {
                let (_, e) = self
                    .peek()
                    .ok_or_else(|| syntax(pos, "trailing class escape"))?;
                self.at += 1;
                if matches!(e, 'p' | 'P') {
                    return Err(syntax(pos, "Unicode property escapes are unsupported"));
                }
                if let Some(class) = match e {
                    'd' => Some(CodeUnitClass::Digit(false)),
                    'D' => Some(CodeUnitClass::Digit(true)),
                    's' => Some(CodeUnitClass::Space(false)),
                    'S' => Some(CodeUnitClass::Space(true)),
                    'w' => Some(CodeUnitClass::Word(false)),
                    'W' => Some(CodeUnitClass::Word(true)),
                    _ => None,
                } {
                    classes.push(class);
                    continue;
                }
                e
            } else {
                ch
            };
            let mut first_units = [0; 2];
            let encoded = first.encode_utf16(&mut first_units);
            if encoded.len() != 1 {
                return Err(syntax(
                    pos,
                    "non-BMP character classes require Unicode set support",
                ));
            }
            let first = encoded[0];
            if self.take('-') && self.peek().is_some_and(|(_, c)| c != ']') {
                let (end_pos, end) = self.peek().unwrap();
                self.at += 1;
                let mut end_units = [0; 2];
                let encoded = end.encode_utf16(&mut end_units);
                if encoded.len() != 1 || first > encoded[0] {
                    return Err(syntax(end_pos, "invalid character-class range"));
                }
                ranges.push((first, encoded[0]));
            } else {
                units.push(first);
            }
        }
        Err(syntax(offset, "unterminated character class"))
    }
    fn peek(&self) -> Option<(usize, char)> {
        self.chars.get(self.at).copied()
    }
    fn take(&mut self, expected: char) -> bool {
        if self.peek().is_some_and(|(_, ch)| ch == expected) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Ref, Symbol};
    use sim_lib_standard_core::{
        BoundedLane, CanonicalObservation, CanonicalOutcome, CharacterizationCapture,
        ScenarioLimits, ScenarioObservationLane, ScenarioSpec, publish_characterization_capture,
    };

    fn case(name: &str, fields: &[(&str, String)]) -> Datum {
        Datum::Node {
            tag: Symbol::qualified("javascript-regexp-characterization", "case/v1"),
            fields: std::iter::once((Symbol::new("name"), Datum::String(name.to_owned())))
                .chain(
                    fields
                        .iter()
                        .map(|(key, value)| (Symbol::new(*key), Datum::String(value.clone()))),
                )
                .collect(),
        }
    }

    fn refusal(source: &str, flags: &str, clause: &str) -> Datum {
        let error = JavascriptRegExp::compile(source, flags).unwrap_err();
        let (class, diagnostic) = match error {
            JavascriptRegExpError::UnsupportedFlag(flag) => {
                ("unsupported-flag", format!("flag {flag} is unsupported"))
            }
            JavascriptRegExpError::UnsupportedSyntax { offset, reason } => {
                ("unsupported-syntax", format!("byte {offset}: {reason}"))
            }
        };
        for roadmap_family in ["PATTERN", "CHARACTERIZE", "ROADMAP"] {
            assert!(!diagnostic.contains(roadmap_family), "{diagnostic}");
        }
        case(
            "refusal",
            &[
                ("clause", clause.to_owned()),
                ("class", class.to_owned()),
                ("diagnostic", diagnostic),
            ],
        )
    }

    fn gap_name(gap: JavascriptRegExpGap) -> &'static str {
        match gap {
            JavascriptRegExpGap::Flags => "flags",
            JavascriptRegExpGap::Backreferences => "backreferences",
            JavascriptRegExpGap::Lookaround => "lookaround",
            JavascriptRegExpGap::UnicodeProperties => "unicode-properties",
            JavascriptRegExpGap::WordBoundary => "word-boundary",
        }
    }
    #[test]
    fn admitted_subset_executes_in_bounded_organ() {
        let r = JavascriptRegExp::compile(r"^[A-Z]+\d?$", "").unwrap();
        assert!(r.find("SIM4", 0, 1000).is_some());
        assert!(r.find("sim", 0, 1000).is_none());
    }
    #[test]
    fn shared_regular_features_report_code_unit_spans() {
        let regexp = JavascriptRegExp::compile(r"^(?:ab|😀){2,3}(c+?)$", "").unwrap();
        let matched = regexp.find("ab😀cc", 0, 10_000).unwrap();
        assert_eq!((matched.start, matched.end), (0, 6));
        assert_eq!(matched.captures, [(4, 6)]);
        assert!(regexp.find("xababcc", 0, 10_000).is_none());
        assert!(
            JavascriptRegExp::compile(r"^[\d]+$", "")
                .unwrap()
                .find("42", 0, 1_000)
                .is_some()
        );
        assert!(
            JavascriptRegExp::compile("^.$", "")
                .unwrap()
                .find("\n", 0, 1_000)
                .is_none()
        );
    }
    #[test]
    fn refused_clause_keeps_its_typed_diagnostic() {
        assert_eq!(
            JavascriptRegExp::compile(r"\bword", ""),
            Err(JavascriptRegExpError::UnsupportedSyntax {
                offset: 0,
                reason: "word-boundary assertions are unsupported",
            })
        );
    }
    #[test]
    fn unsupported_features_fail_closed() {
        for p in [r"(a)\1", r"\p{Letter}", r"\bword", "(?=a)"] {
            assert!(JavascriptRegExp::compile(p, "").is_err(), "{p}");
        }
        assert_eq!(
            JavascriptRegExp::compile("a", "g"),
            Err(JavascriptRegExpError::UnsupportedFlag('g'))
        );
    }
    #[test]
    fn gaps_and_successor_are_blunt() {
        assert_eq!(javascript_regexp_gaps().len(), 5);
        assert_eq!(
            JAVASCRIPT_REGEXP_SUCCESSOR,
            "remaining ECMAScript RegExp clauses"
        );
    }

    #[test]
    fn current_regexp_behavior_is_a_stable_characterization_capture() {
        let unicode = JavascriptRegExp::compile("😀+", "").unwrap();
        let unicode_match = unicode.find("x😀😀y", 0, 1_000).unwrap();
        let greedy = JavascriptRegExp::compile("a*a", "")
            .unwrap()
            .find("aaa", 0, 1_000);
        let lazy = JavascriptRegExp::compile("a*?a", "")
            .unwrap()
            .find("aaa", 0, 1_000);
        let empty = JavascriptRegExp::compile("a*", "")
            .unwrap()
            .find("bbb", 0, 1_000);
        let limited = JavascriptRegExp::compile("a*b", "")
            .unwrap()
            .find("aaab", 0, 1);
        let cases = vec![
            case(
                "unicode-byte-offsets",
                &[(
                    "span",
                    format!("{}..{}", unicode_match.start, unicode_match.end),
                )],
            ),
            case("greedy-repetition", &[("match", format!("{greedy:?}"))]),
            case("lazy-repetition", &[("match", format!("{lazy:?}"))]),
            case("empty-match", &[("match", format!("{empty:?}"))]),
            case(
                "limit-exhaustion",
                &[
                    ("clause", "maximum VM steps".to_owned()),
                    (
                        "outcome",
                        if limited.is_none() {
                            "refused"
                        } else {
                            "matched"
                        }
                        .to_owned(),
                    ),
                ],
            ),
            refusal("a", "g", "flags"),
            refusal(r"\bword", "", "word-boundary"),
            refusal("\\", "", "trailing-escape"),
            refusal("*", "", "quantifier-without-atom"),
        ];
        let scenario = ScenarioSpec::new(
            Symbol::qualified("javascript-regexp-characterization", "current/v1"),
            Symbol::qualified("javascript-regexp-characterization", "shared-text-vm/v1"),
        )
        .with_limits(ScenarioLimits::new(0, cases.len()))
        .observing(ScenarioObservationLane::ValueOrFailure);
        let capture = CharacterizationCapture::new(
            Symbol::qualified("javascript-regexp-characterization", "dialect-cases/v1"),
            CanonicalObservation {
                outcome: Some(CanonicalOutcome::Success(Datum::Vector(cases))),
                events: BoundedLane::Absent,
                receipts: BoundedLane::Absent,
                browse: BoundedLane::Absent,
            },
        );
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let first = publish_characterization_capture(&mut cx, &scenario, &capture).unwrap();
        let replay = publish_characterization_capture(&mut cx, &scenario, &capture).unwrap();
        assert!(matches!(first, Ref::Content(_)));
        assert_eq!(first, replay);
    }

    #[test]
    fn public_gap_data_is_frozen_clause_for_clause() {
        let clauses = javascript_regexp_gaps()
            .iter()
            .copied()
            .map(gap_name)
            .collect::<Vec<_>>();
        assert_eq!(
            clauses,
            [
                "flags",
                "backreferences",
                "lookaround",
                "unicode-properties",
                "word-boundary",
            ]
        );
    }
}
