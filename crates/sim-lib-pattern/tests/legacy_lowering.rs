use sim_lib_pattern::{TextLimits, compile_glob_pattern, compile_lua_pattern, run_text_pattern};

fn span(pattern: &str, subject: &str) -> Option<(usize, usize)> {
    let ops = compile_lua_pattern(pattern).unwrap();
    run_text_pattern(&ops, subject, 0, TextLimits::default())
        .map(|matched| (matched.start, matched.end))
}

#[test]
fn legacy_lua_programs_lower_without_changing_observations() {
    let cases = [
        ("abc", "xxabczz", Some((2, 5))),
        ("^abc", "xxabc", None),
        ("abc$", "xxabc", Some((2, 5))),
        ("%a+", "abc123", Some((0, 3))),
        ("a*a", "aaa", Some((0, 3))),
        ("a-a", "aaa", Some((0, 1))),
        ("a-", "aaa", Some((0, 0))),
        ("%b()", "x(a(b)c)y", Some((1, 8))),
        ("%f[%a]cat", "1cat", Some((1, 4))),
        ("(😀+)", "x😀😀y", Some((1, 9))),
    ];
    for (pattern, subject, expected) in cases {
        assert_eq!(
            span(pattern, subject),
            expected,
            "{pattern:?} over {subject:?}"
        );
    }

    let captures = compile_lua_pattern("(%a+)%s+(%d+)").unwrap();
    let matched = run_text_pattern(&captures, "id 42", 0, TextLimits::default()).unwrap();
    assert_eq!(matched.captures, vec![(0, 2), (3, 5)]);
}

#[test]
fn legacy_glob_programs_use_the_same_lowering() {
    for (pattern, subject, expected) in [
        ("*.rs", "lib.rs", true),
        ("*.rs", "lib.py", false),
        ("src/?ain.rs", "src/main.rs", true),
        ("file[!0-9].txt", "filex.txt", true),
    ] {
        let ops = compile_glob_pattern(pattern).unwrap();
        assert_eq!(
            run_text_pattern(&ops, subject, 0, TextLimits::default()).is_some(),
            expected
        );
    }
}
