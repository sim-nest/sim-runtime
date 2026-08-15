use sim_lib_pattern::{
    AssertionId, ByteDomain, ByteOffset, CaptureId, CodeUnitDomain, CodeUnitOffset, Cursor,
    EnginePolicy, IrError, IrNode, PatternIr, RepeatBounds,
};
use std::collections::BTreeMap;

#[test]
fn cursor_preserves_independent_positions() {
    let byte_cursor = Cursor::<ByteDomain>::new(ByteOffset(3), ByteOffset(11));
    assert_eq!(byte_cursor.source_position(), ByteOffset(3));
    assert_eq!(byte_cursor.subject_position(), ByteOffset(11));

    let code_unit_cursor =
        Cursor::<CodeUnitDomain>::new(CodeUnitOffset::new(2), CodeUnitOffset::new(7));
    assert_eq!(code_unit_cursor.source_position(), CodeUnitOffset::new(2));
    assert_eq!(code_unit_cursor.subject_position(), CodeUnitOffset::new(7));
}

#[test]
fn invalid_repeat_names_both_bounds() {
    let error = RepeatBounds::new(4, Some(3)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid repeat bounds: minimum 4 exceeds maximum 3"
    );
}

#[test]
fn validation_rejects_duplicate_captures_cycles_and_extensions() {
    let capture = |symbol| IrNode::Capture {
        id: CaptureId(7),
        node: Box::new(IrNode::Symbol(symbol)),
    };
    let duplicate = PatternIr::<ByteDomain, &str>::new(
        IrNode::Concat(vec![capture(b'a'), capture(b'b')]),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    );
    assert_eq!(
        duplicate.unwrap_err(),
        IrError::DuplicateCapture(CaptureId(7))
    );

    let assertions = BTreeMap::from([
        (AssertionId(1), IrNode::Assertion(AssertionId(2))),
        (AssertionId(2), IrNode::Assertion(AssertionId(1))),
    ]);
    let cycle = PatternIr::<ByteDomain, &str>::new(
        IrNode::Assertion(AssertionId(1)),
        assertions,
        &EnginePolicy::new([]),
    );
    assert!(matches!(cycle, Err(IrError::AssertionCycle(_))));

    let denied = PatternIr::<ByteDomain, &str>::new(
        IrNode::Extension("backreference"),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    );
    assert!(matches!(denied, Err(IrError::UnsupportedExtension(_))));
}
