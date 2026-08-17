use sim_lib_control::WorkLimit;
use sim_lib_machine::{ShuffleError, ShufflePlan, StackError, UnitStack, ValueWidthPolicy};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Value {
    id: usize,
    width: usize,
}

struct VariableWidth;

impl ValueWidthPolicy for VariableWidth {
    type Value = Value;

    fn width(value: &Value) -> usize {
        value.width
    }
}

#[test]
fn every_layout_through_four_units_has_the_modeled_construction_result() {
    for total_units in 1..=4 {
        for widths in compositions(total_units) {
            for output_len in 0..=4 {
                enumerate_sequences(total_units + 1, output_len, &mut |output| {
                    let expected = modeled_groups(&widths, output);
                    let actual = ShufflePlan::new(widths.clone(), output.iter().copied());
                    assert_eq!(actual.is_ok(), expected.is_ok(), "{widths:?} -> {output:?}");

                    if let (Ok(plan), Ok(groups)) = (actual, expected) {
                        let mut stack = stack(&widths, 16);
                        plan.execute(&mut stack).unwrap();
                        let actual_ids = pop_ids(&mut stack);
                        let expected_ids = groups.into_iter().rev().collect::<Vec<_>>();
                        assert_eq!(actual_ids, expected_ids, "{widths:?} -> {output:?}");
                    }
                });
            }
        }
    }
}

#[test]
fn construction_names_a_two_unit_group_that_would_be_split() {
    assert_eq!(
        ShufflePlan::new([1, 2, 1], [0, 1, 3, 2]),
        Err(ShuffleError::SplitGroup { group: 1 })
    );
}

#[test]
fn failure_after_staging_has_not_changed_the_stack() {
    let plan = ShufflePlan::new([1, 2], [0, 1, 2, 1, 2]).unwrap();
    let mut stack = stack(&[1, 2], 4);

    assert_eq!(
        plan.execute(&mut stack),
        Err(ShuffleError::Stack(StackError::Overflow {
            depth: 3,
            width: 2,
            limit: 4,
        }))
    );
    assert_eq!(stack.depth(), 3);
    assert_eq!(pop_ids(&mut stack), vec![1, 0]);
}

fn stack(widths: &[usize], limit: usize) -> UnitStack<VariableWidth> {
    let mut stack = UnitStack::new(WorkLimit(limit));
    for (id, &width) in widths.iter().enumerate() {
        stack.push(Value { id, width }).unwrap();
    }
    stack
}

fn pop_ids(stack: &mut UnitStack<VariableWidth>) -> Vec<usize> {
    let mut ids = Vec::new();
    while let Ok(value) = stack.pop() {
        ids.push(value.id);
    }
    ids
}

fn compositions(total: usize) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    for cuts in 0..(1_usize << (total - 1)) {
        let mut widths = Vec::new();
        let mut width = 1;
        for boundary in 0..total - 1 {
            if cuts & (1 << boundary) == 0 {
                width += 1;
            } else {
                widths.push(width);
                width = 1;
            }
        }
        widths.push(width);
        result.push(widths);
    }
    result
}

fn enumerate_sequences(alphabet: usize, length: usize, visit: &mut impl FnMut(&[usize])) {
    fn recurse(
        alphabet: usize,
        length: usize,
        sequence: &mut Vec<usize>,
        visit: &mut impl FnMut(&[usize]),
    ) {
        if sequence.len() == length {
            visit(sequence);
            return;
        }
        for unit in 0..alphabet {
            sequence.push(unit);
            recurse(alphabet, length, sequence, visit);
            sequence.pop();
        }
    }
    recurse(alphabet, length, &mut Vec::new(), visit);
}

fn modeled_groups(widths: &[usize], output: &[usize]) -> Result<Vec<usize>, ()> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for &width in widths {
        ranges.push(start..start + width);
        start += width;
    }

    let mut groups = Vec::new();
    let mut cursor = 0;
    while cursor < output.len() {
        let Some(group) = ranges
            .iter()
            .position(|range| range.start == output[cursor])
        else {
            return Err(());
        };
        let range = ranges[group].clone();
        let end = cursor + range.len();
        if output
            .get(cursor..end)
            .is_none_or(|units| !units.iter().copied().eq(range))
        {
            return Err(());
        }
        groups.push(group);
        cursor = end;
    }
    Ok(groups)
}
