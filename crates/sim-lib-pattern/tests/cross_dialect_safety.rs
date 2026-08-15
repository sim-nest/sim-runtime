use sim_lib_pattern::{
    ByteDomain, ByteOffset, CaptureId, CodeUnitDomain, CodeUnitOffset, DomainExecutionOutcome,
    EnginePolicy, ExecutionLimit, GlobPatternDialect, IrNode, LuaPatternDialect, PatternIr,
    RepeatBounds, ScalarDomain, ScalarOffset, TextLimits, compile, execute_bytes,
    execute_code_units, execute_scalars,
};
use sim_text::CodeUnitString;
use std::collections::BTreeMap;

fn limits(max_steps: usize) -> TextLimits {
    TextLimits {
        max_steps,
        ..TextLimits::default()
    }
}

#[test]
fn one_organ_executes_three_distinct_surface_lowerings() {
    let neutral = PatternIr::<ByteDomain, ()>::new(
        IrNode::Repeat {
            node: Box::new(IrNode::Symbol(b'a')),
            bounds: RepeatBounds::new(1, None).unwrap(),
            greedy: true,
        },
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let neutral = execute_bytes(&compile(&neutral), b"aaa", limits(128), |_, _| false);
    let DomainExecutionOutcome::Match { matched, .. } = neutral else {
        panic!("neutral IR must match through the shared organ");
    };
    assert_eq!((matched.start, matched.end), (ByteOffset(0), ByteOffset(3)));

    let lua = LuaPatternDialect.compile_ir("^a+$").unwrap();
    let lua = execute_scalars(&compile(&lua), &['a', 'a', 'a'], limits(128), |_, _| false);
    let DomainExecutionOutcome::Match { matched, .. } = lua else {
        panic!("Lua syntax must match through the shared organ");
    };
    assert_eq!(
        (matched.start, matched.end),
        (ScalarOffset::new(0), ScalarOffset::new(3))
    );

    let glob = GlobPatternDialect.compile_ir("a*").unwrap();
    let glob = execute_scalars(&compile(&glob), &['a', 'a', 'a'], limits(128), |_, _| false);
    let DomainExecutionOutcome::Match { matched, .. } = glob else {
        panic!("glob syntax must match through the shared organ");
    };
    assert_eq!(
        (matched.start, matched.end),
        (ScalarOffset::new(0), ScalarOffset::new(3))
    );
}

#[test]
fn adversarial_ambiguity_stops_with_an_exact_bounded_work_receipt() {
    let ambiguous = IrNode::Repeat {
        node: Box::new(IrNode::Alternation(vec![
            IrNode::Symbol(b'a'),
            IrNode::Concat(vec![IrNode::Symbol(b'a'), IrNode::Symbol(b'a')]),
        ])),
        bounds: RepeatBounds::new(0, None).unwrap(),
        greedy: true,
    };
    let ir = PatternIr::<ByteDomain, ()>::new(
        IrNode::Concat(vec![ambiguous, IrNode::Symbol(b'b')]),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let outcome = execute_bytes(&compile(&ir), &vec![b'a'; 256], limits(40), |_, _| false);
    let DomainExecutionOutcome::Limit { limit, receipt } = outcome else {
        panic!("ambiguous rejection must stop at the caller's work boundary");
    };
    assert_eq!(limit, ExecutionLimit::Transitions);
    assert_eq!(receipt.transitions, 40);
    assert!(receipt.state_visits <= 40);
    assert_eq!(receipt.subject_symbols, 256);
}

#[test]
fn offsets_remain_exact_in_all_three_subject_domains() {
    let bytes = PatternIr::<ByteDomain, ()>::new(
        IrNode::Concat(
            "😀x"
                .as_bytes()
                .iter()
                .copied()
                .map(IrNode::Symbol)
                .collect(),
        ),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let DomainExecutionOutcome::Match { matched: bytes, .. } =
        execute_bytes(&compile(&bytes), "😀x".as_bytes(), limits(128), |_, _| {
            false
        })
    else {
        panic!("byte lowering must match");
    };

    let scalars = PatternIr::<ScalarDomain, ()>::new(
        IrNode::Concat(vec![IrNode::Symbol('😀'), IrNode::Symbol('x')]),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let DomainExecutionOutcome::Match {
        matched: scalars, ..
    } = execute_scalars(&compile(&scalars), &['😀', 'x'], limits(128), |_, _| {
        false
    })
    else {
        panic!("scalar lowering must match");
    };

    let units = CodeUnitString::from_scalar("😀x");
    let code_units = PatternIr::<CodeUnitDomain, ()>::new(
        IrNode::Concat(
            units
                .as_code_units()
                .iter()
                .copied()
                .map(IrNode::Symbol)
                .collect(),
        ),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let DomainExecutionOutcome::Match {
        matched: code_units,
        ..
    } = execute_code_units(&compile(&code_units), &units, limits(128), |_, _| false)
    else {
        panic!("code-unit lowering must match");
    };

    assert_eq!(bytes.end, ByteOffset(5));
    assert_eq!(scalars.end, ScalarOffset::new(2));
    assert_eq!(code_units.end, CodeUnitOffset::new(3));
}

#[test]
fn ordered_capture_precedence_is_stable_and_unsupported_syntax_fails_closed() {
    let first = CaptureId(10);
    let second = CaptureId(20);
    let ir = PatternIr::<ByteDomain, ()>::new(
        IrNode::Alternation(vec![
            IrNode::Capture {
                id: first,
                node: Box::new(IrNode::Symbol(b'a')),
            },
            IrNode::Capture {
                id: second,
                node: Box::new(IrNode::Symbol(b'a')),
            },
        ]),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    let DomainExecutionOutcome::Match { matched, .. } =
        execute_bytes(&compile(&ir), b"a", limits(128), |_, _| false)
    else {
        panic!("ordered ambiguity must match");
    };
    assert_eq!(
        matched.captures.keys().copied().collect::<Vec<_>>(),
        [first]
    );

    assert!(LuaPatternDialect.compile_ir("[unterminated").is_err());
    assert!(GlobPatternDialect.compile_ir("[unterminated").is_err());
}
