// conformance: pattern cursors retain their byte, scalar, and code-unit domains.

use sim_lib_pattern::{
    CodeUnitDomain, CodeUnitOffset, DomainExecutionOutcome, EnginePolicy, IrNode, PatternIr,
    ScalarDomain, ScalarOffset, TextLimits, compile, execute_code_units, execute_scalars,
};
use sim_text::CodeUnitString;
use std::collections::BTreeMap;

fn compiled<D: sim_lib_pattern::SymbolDomain>(
    symbol: D::Symbol,
) -> sim_lib_pattern::Automaton<D::Symbol, ()>
where
    D::Symbol: Clone,
{
    let ir = PatternIr::<D, ()>::new(
        IrNode::Symbol(symbol),
        BTreeMap::new(),
        &EnginePolicy::new([]),
    )
    .unwrap();
    compile(&ir)
}

#[test]
fn code_unit_match_indexes_lone_surrogate_exactly() {
    let subject = CodeUnitString::from_code_units(vec![0xd800]);
    let outcome = execute_code_units(
        &compiled::<CodeUnitDomain>(0xd800),
        &subject,
        TextLimits::default(),
        |_, _| false,
    );
    let DomainExecutionOutcome::Match { matched, receipt } = outcome else {
        panic!("lone surrogate must remain matchable as one exact code unit");
    };
    assert_eq!(matched.start, CodeUnitOffset::new(0));
    assert_eq!(matched.end, CodeUnitOffset::new(1));
    assert_eq!(subject.code_unit_at(matched.start), Some(0xd800));
    assert_eq!(receipt.subject_symbols, 1);
}

#[test]
fn code_unit_cursor_can_stop_between_surrogate_halves() {
    let subject = CodeUnitString::from_scalar("\u{1f600}");
    let outcome = execute_code_units(
        &compiled::<CodeUnitDomain>(0xd83d),
        &subject,
        TextLimits::default(),
        |_, _| false,
    );
    let DomainExecutionOutcome::Match { matched, .. } = outcome else {
        panic!("must match");
    };
    assert_eq!(matched.end, CodeUnitOffset::new(1));
    assert!(subject.scalar_offset(matched.end).is_err());

    let scalars = ['\u{1f600}'];
    let scalar_outcome = execute_scalars(
        &compiled::<ScalarDomain>('\u{1f600}'),
        &scalars,
        TextLimits::default(),
        |_, _| false,
    );
    let DomainExecutionOutcome::Match { matched, .. } = scalar_outcome else {
        panic!("must match");
    };
    assert_eq!(matched.end, ScalarOffset::new(1));
}
