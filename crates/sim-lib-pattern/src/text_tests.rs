use crate::{
    TextLimits, TextMatch, TextOp, compile_glob_pattern, compile_lua_pattern, run_text_pattern,
};
use std::sync::Arc;

use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Ref, Symbol};
use sim_lib_standard_core::{
    BoundedLane, CanonicalObservation, CanonicalOutcome, CharacterizationCapture, ScenarioLimits,
    ScenarioObservationLane, ScenarioSpec, publish_characterization_capture,
};

fn case(name: &str, fields: &[(&str, &str)]) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("pattern-characterization", "case/v1"),
        fields: std::iter::once((Symbol::new("name"), Datum::String(name.to_owned())))
            .chain(
                fields
                    .iter()
                    .map(|(key, value)| (Symbol::new(*key), Datum::String((*value).to_owned()))),
            )
            .collect(),
    }
}

fn capture(name: &str, cases: Vec<Datum>) -> (ScenarioSpec, CharacterizationCapture) {
    let scenario = ScenarioSpec::new(
        Symbol::qualified("pattern-characterization", name),
        Symbol::qualified("pattern-characterization", "current/v1"),
    )
    .with_limits(ScenarioLimits::new(0, cases.len()))
    .observing(ScenarioObservationLane::ValueOrFailure);
    let capture = CharacterizationCapture::new(
        Symbol::qualified("pattern-characterization", "dialect-cases/v1"),
        CanonicalObservation {
            outcome: Some(CanonicalOutcome::Success(Datum::Vector(cases))),
            events: BoundedLane::Absent,
            receipts: BoundedLane::Absent,
            browse: BoundedLane::Absent,
        },
    );
    (scenario, capture)
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn assert_stable_capture(name: &str, cases: Vec<Datum>) {
    let (scenario, capture) = capture(name, cases);
    let first = publish_characterization_capture(&mut test_cx(), &scenario, &capture).unwrap();
    let replay = publish_characterization_capture(&mut test_cx(), &scenario, &capture).unwrap();
    assert!(matches!(first, Ref::Content(_)));
    assert_eq!(first, replay);
}

fn text_match(ops: &[TextOp], subject: &str) -> Option<TextMatch> {
    run_text_pattern(ops, subject, 0, TextLimits { max_steps: 20_000 })
}

fn text_span(ops: &[TextOp], subject: &str) -> Option<(usize, usize)> {
    text_match(ops, subject).map(|matched| (matched.start, matched.end))
}

#[test]
fn lua_dialect_runs_over_shared_text_vm_table() {
    let cases = [
        ("abc", "abc", Some((0, 3))),
        ("abc", "xxabczz", Some((2, 5))),
        ("^abc", "abczz", Some((0, 3))),
        ("^abc", "xxabc", None),
        ("abc$", "xxabc", Some((2, 5))),
        ("abc$", "abcxx", None),
        (".", "x", Some((0, 1))),
        ("a.c", "abc", Some((0, 3))),
        ("%a+", "abc123", Some((0, 3))),
        ("%d+", "abc123", Some((3, 6))),
        ("%l+", "abcDEF", Some((0, 3))),
        ("%u+", "abcDEF", Some((3, 6))),
        ("%w+", "ab12!!", Some((0, 4))),
        ("%s+", "xx \t yy", Some((2, 5))),
        ("%p+", "abc!?z", Some((3, 5))),
        ("%x+", "g1afz", Some((1, 4))),
        ("%A+", "123abc", Some((0, 3))),
        ("%D+", "abc123", Some((0, 3))),
        ("%L+", "ABCabc", Some((0, 3))),
        ("%U+", "abcABC", Some((0, 3))),
        ("%W+", "!!abc", Some((0, 2))),
        ("%S+", "ab cd", Some((0, 2))),
        ("%P+", "ab!cd", Some((0, 2))),
        ("%X+", "zzaf", Some((0, 2))),
        ("[abc]+", "zzcab", Some((2, 5))),
        ("[^abc]+", "abc123", Some((3, 6))),
        ("[a-c]+", "xxabc", Some((2, 5))),
        ("[%d]+", "aa123", Some((2, 5))),
        ("a*", "aaab", Some((0, 3))),
        ("a+", "baaac", Some((1, 4))),
        ("a?b", "ab", Some((0, 2))),
        ("a?b", "b", Some((0, 1))),
        ("a-b", "aaab", Some((0, 4))),
        ("a-", "aaa", Some((0, 0))),
        ("%b()", "x(a(b)c)y", Some((1, 8))),
        ("%f[%a]cat", "1cat", Some((1, 4))),
        ("%f[%d]%d+", "ab123", Some((2, 5))),
        ("%f[^%a]123", "abc123", Some((3, 6))),
        ("%%", "a%b", Some((1, 2))),
        ("%.", "a.b", Some((1, 2))),
        ("%z", "a\0b", Some((1, 2))),
        ("()abc()", "abc", Some((0, 3))),
        ("(a+)", "aa", Some((0, 2))),
        ("a^b", "a^b", Some((0, 3))),
        ("$x", "$x", Some((0, 2))),
        ("^$", "", Some((0, 0))),
        ("%a*%d", "abc1", Some((0, 4))),
        ("%a-%d", "abc1", Some((0, 4))),
        ("colou?r", "color", Some((0, 5))),
        ("colou?r", "colour", Some((0, 6))),
    ];

    assert!(cases.len() >= 40);
    for (pattern, subject, expected) in cases {
        let ops = compile_lua_pattern(pattern).unwrap();
        assert_eq!(
            text_span(&ops, subject),
            expected,
            "pattern {pattern:?} subject {subject:?}"
        );
    }
}

#[test]
fn lua_dialect_preserves_captures_and_budget_limits() {
    let ops = compile_lua_pattern("(%a+)%s+(%d+)").unwrap();
    let matched = text_match(&ops, "id 42").unwrap();
    assert_eq!((matched.start, matched.end), (0, 5));
    assert_eq!(matched.captures, vec![(0, 2), (3, 5)]);

    let empty = compile_lua_pattern("()abc()").unwrap();
    let matched = text_match(&empty, "abc").unwrap();
    assert_eq!(matched.captures, vec![(0, 0), (3, 3)]);

    let bounded = compile_lua_pattern("a*b").unwrap();
    assert!(run_text_pattern(&bounded, "aaab", 0, TextLimits { max_steps: 1 }).is_none());
    assert_eq!(text_span(&bounded, "aaab"), Some((0, 4)));
}

#[test]
fn glob_dialect_reuses_the_same_text_vm() {
    let cases = [
        ("*.rs", "lib.rs", true),
        ("*.rs", "lib.py", false),
        ("src/?ain.rs", "src/main.rs", true),
        ("src/?ain.rs", "src/plain.rs", false),
        ("file[0-9].txt", "file7.txt", true),
        ("file[!0-9].txt", "filex.txt", true),
        ("file[!0-9].txt", "file7.txt", false),
        ("literal\\*.txt", "literal*.txt", true),
        ("a[bc]d", "acd", true),
        ("a[bc]d", "aed", false),
    ];

    for (pattern, subject, expected) in cases {
        let ops = compile_glob_pattern(pattern).unwrap();
        assert_eq!(
            text_match(&ops, subject).is_some(),
            expected,
            "glob {pattern:?} subject {subject:?}"
        );
    }
}

#[test]
fn text_pattern_dialects_fail_closed_on_malformed_patterns() {
    assert!(compile_lua_pattern("*").is_err());
    assert!(compile_lua_pattern("[abc").is_err());
    assert!(compile_lua_pattern("%").is_err());
    assert!(compile_glob_pattern("[abc").is_err());
}

#[test]
fn lua_current_behavior_is_a_stable_characterization_capture() {
    let unicode = compile_lua_pattern("(😀+)").unwrap();
    let unicode_match = text_match(&unicode, "x😀😀y").unwrap();
    let greedy = text_span(&compile_lua_pattern("a*a").unwrap(), "aaa");
    let lazy = text_span(&compile_lua_pattern("a-a").unwrap(), "aaa");
    let empty = text_span(&compile_lua_pattern("a-").unwrap(), "aaa");
    let bounded = run_text_pattern(
        &compile_lua_pattern("a*b").unwrap(),
        "aaab",
        0,
        TextLimits { max_steps: 1 },
    );
    let refusals = [
        ("quantifier-without-atom", "*"),
        ("unterminated-character-set", "[abc"),
        ("dangling-percent-escape", "%"),
    ];
    let mut cases = vec![
        case(
            "unicode-byte-offsets",
            &[
                (
                    "span",
                    &format!("{}..{}", unicode_match.start, unicode_match.end),
                ),
                (
                    "capture",
                    &format!(
                        "{}..{}",
                        unicode_match.captures[0].0, unicode_match.captures[0].1
                    ),
                ),
            ],
        ),
        case("greedy-repetition", &[("span", &format!("{greedy:?}"))]),
        case("lazy-repetition", &[("span", &format!("{lazy:?}"))]),
        case("empty-match", &[("span", &format!("{empty:?}"))]),
        case(
            "limit-exhaustion",
            &[
                (
                    "outcome",
                    if bounded.is_none() {
                        "refused"
                    } else {
                        "matched"
                    },
                ),
                ("clause", "maximum VM steps"),
            ],
        ),
    ];
    for (clause, pattern) in refusals {
        let diagnostic = compile_lua_pattern(pattern).unwrap_err().to_string();
        assert!(
            diagnostic.contains(clause.replace('-', " ").as_str())
                || diagnostic.contains("quantifier without atom")
        );
        assert!(!diagnostic.contains("PATTERN"));
        cases.push(case(
            "malformed-program",
            &[("clause", clause), ("diagnostic", &diagnostic)],
        ));
    }
    assert_stable_capture("lua/v1", cases);
}

#[test]
fn glob_current_behavior_is_a_stable_characterization_capture() {
    let unicode = text_span(&compile_glob_pattern("?😀*").unwrap(), "å😀x");
    let empty = text_span(&compile_glob_pattern("*").unwrap(), "");
    let rejected = text_span(&compile_glob_pattern("*.rs").unwrap(), "lib.py");
    let limited = run_text_pattern(
        &compile_glob_pattern("*x").unwrap(),
        "abcx",
        0,
        TextLimits { max_steps: 1 },
    );
    let diagnostics = [
        (
            "unterminated-character-set",
            "unterminated glob character set",
            compile_glob_pattern("[abc").unwrap_err().to_string(),
        ),
        (
            "dangling-escape",
            "dangling escape",
            compile_glob_pattern("\\").unwrap_err().to_string(),
        ),
    ];
    let mut cases = vec![
        case("unicode-byte-offsets", &[("span", &format!("{unicode:?}"))]),
        case("empty-match", &[("span", &format!("{empty:?}"))]),
        case(
            "ordinary-rejection",
            &[(
                "outcome",
                if rejected.is_none() {
                    "no-match"
                } else {
                    "matched"
                },
            )],
        ),
        case(
            "limit-exhaustion",
            &[
                (
                    "outcome",
                    if limited.is_none() {
                        "refused"
                    } else {
                        "matched"
                    },
                ),
                ("clause", "maximum VM steps"),
            ],
        ),
    ];
    for (clause, expected_detail, diagnostic) in diagnostics {
        assert!(diagnostic.contains(expected_detail));
        assert!(!diagnostic.contains("PATTERN"));
        cases.push(case(
            "malformed-program",
            &[("clause", clause), ("diagnostic", &diagnostic)],
        ));
    }
    assert_stable_capture("glob/v1", cases);
}
