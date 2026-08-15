use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_machine::{SlotError, SlotFile, StackError, UnitStack, ValueWidthPolicy};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Value {
    id: u16,
    width: usize,
}

struct OneUnit;
struct TwoUnits;
struct VariableUnits;

impl ValueWidthPolicy for OneUnit {
    type Value = Value;
    fn width(_: &Value) -> usize {
        1
    }
}

impl ValueWidthPolicy for TwoUnits {
    type Value = Value;
    fn width(_: &Value) -> usize {
        2
    }
}

impl ValueWidthPolicy for VariableUnits {
    type Value = Value;
    fn width(value: &Value) -> usize {
        value.width
    }
}

#[test]
fn two_unit_value_is_never_half_initialized() {
    let mut slots = SlotFile::<TwoUnits>::new(AdmissionLimit(4));
    slots.store(0, value(1, 2)).unwrap();
    slots.store(1, value(2, 2)).unwrap();

    assert_eq!(slots.load(0), Err(SlotError::Uninitialized { slot: 0 }));
    assert!(!slots.is_initialized(0));
    assert!(slots.is_initialized(1));
    assert!(slots.is_initialized(2));
    assert_eq!(slots.load(1).unwrap().id, 2);
    assert_eq!(slots.load(2), Err(SlotError::Uninitialized { slot: 2 }));
}

#[test]
fn failures_report_exact_slot_depth_width_and_limit() {
    let mut slots = SlotFile::<TwoUnits>::new(AdmissionLimit(3));
    assert_eq!(
        slots.store(2, value(7, 2)),
        Err(SlotError::Overflow {
            slot: 2,
            width: 2,
            limit: 3,
        })
    );

    let mut stack = UnitStack::<TwoUnits>::new(WorkLimit(3));
    assert_eq!(stack.pop(), Err(StackError::Underflow { depth: 0 }));
    stack.push(value(1, 2)).unwrap();
    assert_eq!(
        stack.push(value(2, 2)),
        Err(StackError::Overflow {
            depth: 2,
            width: 2,
            limit: 3,
        })
    );
}

#[test]
fn release_clears_every_unit_before_reuse() {
    let mut slots = SlotFile::<VariableUnits>::new(AdmissionLimit(6));
    slots.store(1, value(9, 3)).unwrap();
    assert_eq!(slots.release(2).unwrap().id, 9);
    for slot in 1..4 {
        assert!(!slots.is_initialized(slot));
        assert_eq!(slots.load(slot), Err(SlotError::Uninitialized { slot }));
    }

    slots.store(2, value(10, 1)).unwrap();
    assert_eq!(slots.load(2).unwrap().id, 10);
}

#[test]
fn storage_properties_hold_for_one_unit_policy() {
    exercise::<OneUnit>(1);
}

#[test]
fn storage_properties_hold_for_two_unit_policy() {
    exercise::<TwoUnits>(2);
}

#[test]
fn storage_properties_hold_for_variable_unit_policy() {
    exercise::<VariableUnits>(0);
}

fn exercise<P: ValueWidthPolicy<Value = Value>>(fixed_width: usize) {
    for limit in 1..=17 {
        let mut stack = UnitStack::<P>::new(WorkLimit(limit));
        let mut model = Vec::<Value>::new();
        let mut seed = limit as u64;

        for id in 0..400_u16 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            if seed & 3 == 0 {
                let actual = stack.pop();
                let expected = model.pop();
                match expected {
                    Some(expected) => assert_eq!(actual.unwrap(), expected),
                    None => assert_eq!(actual, Err(StackError::Underflow { depth: 0 })),
                }
            } else {
                let width = if fixed_width == 0 {
                    (seed as usize % 4) + 1
                } else {
                    fixed_width
                };
                let candidate = value(id, width);
                let model_depth = model.iter().map(P::width).sum::<usize>();
                let actual = stack.push(candidate.clone());
                if model_depth + P::width(&candidate) <= limit {
                    actual.unwrap();
                    model.push(candidate);
                } else {
                    assert_eq!(
                        actual,
                        Err(StackError::Overflow {
                            depth: model_depth,
                            width: P::width(&candidate),
                            limit,
                        })
                    );
                }
            }
            assert_eq!(stack.depth(), model.iter().map(P::width).sum());
            assert_eq!(stack.top().ok(), model.last());
        }

        exercise_slots::<P>(limit, fixed_width);
    }
}

fn exercise_slots<P: ValueWidthPolicy<Value = Value>>(limit: usize, fixed_width: usize) {
    let mut slots = SlotFile::<P>::new(AdmissionLimit(limit));
    let mut model = vec![None::<(usize, Value)>; limit];
    let mut seed = (limit as u64) << 8;

    for id in 0..400_u16 {
        seed = seed
            .wrapping_mul(2_862_933_555_777_941_757)
            .wrapping_add(3_037_000_493);
        let slot = seed as usize % limit;
        if seed & 7 == 0 {
            let expected = model[slot].clone();
            let actual = slots.release(slot);
            if let Some((start, expected)) = expected {
                let width = P::width(&expected);
                assert_eq!(actual.unwrap(), expected);
                model[start..start + width].fill(None);
            } else {
                assert_eq!(actual, Err(SlotError::Uninitialized { slot }));
            }
        } else {
            let width = if fixed_width == 0 {
                ((seed >> 16) as usize % 4) + 1
            } else {
                fixed_width
            };
            let candidate = value(id, width);
            let actual = slots.store(slot, candidate.clone());
            let policy_width = P::width(&candidate);
            if slot + policy_width > limit {
                assert_eq!(
                    actual,
                    Err(SlotError::Overflow {
                        slot,
                        width: policy_width,
                        limit,
                    })
                );
            } else {
                actual.unwrap();
                let mut starts = model[slot..slot + policy_width]
                    .iter()
                    .flatten()
                    .map(|(start, _)| *start)
                    .collect::<Vec<_>>();
                starts.sort_unstable();
                starts.dedup();
                for start in starts {
                    let old_width = P::width(&model[start].as_ref().unwrap().1);
                    model[start..start + old_width].fill(None);
                }
                model[slot..slot + policy_width].fill(Some((slot, candidate.clone())));
            }
        }

        for (index, modeled) in model.iter().enumerate() {
            assert_eq!(slots.is_initialized(index), modeled.is_some());
            let expected = modeled
                .as_ref()
                .filter(|(start, _)| *start == index)
                .map(|(_, value)| value);
            assert_eq!(slots.load(index).ok(), expected);
        }
    }
}

fn value(id: u16, width: usize) -> Value {
    Value { id, width }
}
