use crate::{
    TextLimits, TextMatch, TextOp, compile_glob_pattern, compile_lua_pattern, run_text_pattern,
};

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
