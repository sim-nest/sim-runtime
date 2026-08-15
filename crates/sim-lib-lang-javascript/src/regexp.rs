//! Faithful, deliberately narrow ECMAScript RegExp compiler.

use sim_lib_pattern::{TextClass, TextLimits, TextMatch, TextOp, run_text_pattern};

/// Required successor for usable ECMAScript regular expressions.
pub const JAVASCRIPT_REGEXP_SUCCESSOR: &str = "JAVA_SCRIPT_6 pattern-engine work";

/// An explicitly unsupported ECMAScript RegExp clause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptRegExpGap {
    /// Any flag (`d g i m s u v y`) is currently rejected; no stateful `lastIndex` is emulated.
    Flags,
    /// Alternation.
    Alternation,
    /// Capturing and noncapturing groups.
    Groups,
    /// Backreferences.
    Backreferences,
    /// Lookahead or lookbehind.
    Lookaround,
    /// Unicode property escapes and Unicode-set notation.
    UnicodeProperties,
    /// Word-boundary assertions.
    WordBoundary,
    /// Counted quantifiers (`{m,n}`).
    CountedQuantifiers,
}
/// Exact first-release gap manifest.
pub const fn javascript_regexp_gaps() -> &'static [JavascriptRegExpGap] {
    &[
        JavascriptRegExpGap::Flags,
        JavascriptRegExpGap::Alternation,
        JavascriptRegExpGap::Groups,
        JavascriptRegExpGap::Backreferences,
        JavascriptRegExpGap::Lookaround,
        JavascriptRegExpGap::UnicodeProperties,
        JavascriptRegExpGap::WordBoundary,
        JavascriptRegExpGap::CountedQuantifiers,
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
    ops: Vec<TextOp>,
}
impl JavascriptRegExp {
    /// Compile the v1 intersection: literals, `.`, `^`, `$`, simple classes,
    /// `\d \D \s \S \w \W`, and greedy/lazy `? * +` quantifiers.
    pub fn compile(source: &str, flags: &str) -> Result<Self, JavascriptRegExpError> {
        if let Some(flag) = flags.chars().next() {
            return Err(JavascriptRegExpError::UnsupportedFlag(flag));
        }
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        let mut at = 0;
        let mut ops = Vec::new();
        while at < chars.len() {
            let (offset, ch) = chars[at];
            match ch {
                '^' if at == 0 => ops.push(TextOp::AnchorStart),
                '$' if at + 1 == chars.len() => ops.push(TextOp::AnchorEnd),
                '.' => ops.push(TextOp::Any),
                '[' => {
                    let (class, next) = compile_class(&chars, at)?;
                    ops.push(TextOp::Class(class));
                    at = next - 1;
                }
                '\\' => {
                    at += 1;
                    let Some((_, escaped)) = chars.get(at).copied() else {
                        return Err(syntax(offset, "trailing escape"));
                    };
                    ops.push(escape_atom(escaped, offset)?);
                }
                '*' | '+' | '?' => {
                    let (min, max) = match ch {
                        '*' => (0, None),
                        '+' => (1, None),
                        '?' => (0, Some(1)),
                        _ => unreachable!(),
                    };
                    if !matches!(
                        ops.last(),
                        Some(TextOp::Literal(_) | TextOp::Any | TextOp::Class(_))
                    ) {
                        return Err(syntax(offset, "quantifier has no admissible atom"));
                    }
                    let lazy = chars.get(at + 1).is_some_and(|(_, c)| *c == '?');
                    ops.push(TextOp::Repeat {
                        min,
                        max,
                        greedy: !lazy,
                    });
                    if lazy {
                        at += 1;
                    }
                }
                '|' | '(' | ')' | '{' | '}' => {
                    return Err(syntax(
                        offset,
                        "syntax requires JAVA_SCRIPT_6 pattern-engine work",
                    ));
                }
                _ => ops.push(TextOp::Literal(ch)),
            }
            at += 1;
        }
        Ok(Self {
            source: source.into(),
            ops,
        })
    }
    /// Original source.
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Inspect the shared-VM program.
    pub fn ops(&self) -> &[TextOp] {
        &self.ops
    }
    /// Execute under an explicit shared-VM step bound.
    pub fn find(&self, subject: &str, init: usize, max_steps: usize) -> Option<TextMatch> {
        run_text_pattern(&self.ops, subject, init, TextLimits { max_steps })
    }
}
fn syntax(offset: usize, reason: &'static str) -> JavascriptRegExpError {
    JavascriptRegExpError::UnsupportedSyntax { offset, reason }
}
fn escape_atom(ch: char, offset: usize) -> Result<TextOp, JavascriptRegExpError> {
    Ok(match ch {
        'd' => TextOp::Class(TextClass::Digit),
        'D' => TextOp::Class(TextClass::Not(Box::new(TextClass::Digit))),
        's' => TextOp::Class(TextClass::Space),
        'S' => TextOp::Class(TextClass::Not(Box::new(TextClass::Space))),
        'w' => TextOp::Class(TextClass::Alnum),
        'W' => TextOp::Class(TextClass::Not(Box::new(TextClass::Alnum))),
        'b' | 'B' => return Err(syntax(offset, "word-boundary assertions are unsupported")),
        '1'..='9' => return Err(syntax(offset, "backreferences are unsupported")),
        'p' | 'P' => return Err(syntax(offset, "Unicode property escapes are unsupported")),
        other => TextOp::Literal(other),
    })
}
fn compile_class(
    chars: &[(usize, char)],
    start: usize,
) -> Result<(TextClass, usize), JavascriptRegExpError> {
    let offset = chars[start].0;
    let mut at = start + 1;
    let negated = chars.get(at).is_some_and(|(_, c)| *c == '^');
    if negated {
        at += 1;
    }
    let mut literals = Vec::new();
    let mut ranges = Vec::new();
    let mut classes = Vec::new();
    while let Some((pos, ch)) = chars.get(at).copied() {
        if ch == ']' && at > start + 1 {
            return Ok((
                TextClass::Set {
                    chars: literals,
                    ranges,
                    classes,
                    negated,
                },
                at + 1,
            ));
        }
        let atom = if ch == '\\' {
            at += 1;
            let Some((_, e)) = chars.get(at).copied() else {
                return Err(syntax(pos, "trailing class escape"));
            };
            match escape_atom(e, pos)? {
                TextOp::Class(c) => {
                    classes.push(c);
                    None
                }
                TextOp::Literal(c) => Some(c),
                _ => None,
            }
        } else {
            Some(ch)
        };
        if let Some(first) = atom {
            if chars.get(at + 1).is_some_and(|(_, c)| *c == '-')
                && chars.get(at + 2).is_some_and(|(_, c)| *c != ']')
            {
                let end = chars[at + 2].1;
                if first > end {
                    return Err(syntax(pos, "descending character-class range"));
                }
                ranges.push((first, end));
                at += 2;
            } else {
                literals.push(first);
            }
        }
        at += 1;
    }
    Err(syntax(offset, "unterminated character class"))
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
            JavascriptRegExpGap::Alternation => "alternation",
            JavascriptRegExpGap::Groups => "groups",
            JavascriptRegExpGap::Backreferences => "backreferences",
            JavascriptRegExpGap::Lookaround => "lookaround",
            JavascriptRegExpGap::UnicodeProperties => "unicode-properties",
            JavascriptRegExpGap::WordBoundary => "word-boundary",
            JavascriptRegExpGap::CountedQuantifiers => "counted-quantifiers",
        }
    }
    #[test]
    fn admitted_subset_executes_in_bounded_organ() {
        let r = JavascriptRegExp::compile(r"^[A-Z]+\d?$", "").unwrap();
        assert!(r.find("SIM4", 0, 1000).is_some());
        assert!(r.find("sim", 0, 1000).is_none());
    }
    #[test]
    fn unsupported_features_fail_closed() {
        for p in ["a|b", "(a)", r"(a)\1", r"\p{Letter}", r"\bword"] {
            assert!(JavascriptRegExp::compile(p, "").is_err(), "{p}");
        }
        assert_eq!(
            JavascriptRegExp::compile("a", "g"),
            Err(JavascriptRegExpError::UnsupportedFlag('g'))
        );
    }
    #[test]
    fn gaps_and_successor_are_blunt() {
        assert_eq!(javascript_regexp_gaps().len(), 8);
        assert_eq!(
            JAVASCRIPT_REGEXP_SUCCESSOR,
            "JAVA_SCRIPT_6 pattern-engine work"
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
                "alternation",
                "groups",
                "backreferences",
                "lookaround",
                "unicode-properties",
                "word-boundary",
                "counted-quantifiers",
            ]
        );
    }
}
