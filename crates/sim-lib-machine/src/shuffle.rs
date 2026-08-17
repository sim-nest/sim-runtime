use crate::{StackError, UnitStack, ValueWidthPolicy};

/// Failure to construct or execute a logical stack shuffle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShuffleError {
    /// A logical value group has no units.
    ZeroWidthGroup {
        /// Index of the invalid group, counted from the bottom of the stack.
        group: usize,
    },
    /// An output unit does not name a unit in the input layout.
    UnknownUnit {
        /// Invalid logical input-unit index.
        unit: usize,
    },
    /// An output layout selects only part of a value group or changes its unit order.
    SplitGroup {
        /// Index of the offending group, counted from the bottom of the stack.
        group: usize,
    },
    /// The live stack does not have the layout for which the plan was validated.
    LayoutMismatch {
        /// First expected group that was absent or had a different width.
        group: usize,
        /// Width recorded in the plan, or zero when the plan has no such group.
        expected: usize,
        /// Width found on the stack, or zero when the stack has no such group.
        actual: usize,
    },
    /// The staged output cannot be represented by the operand stack.
    Stack(StackError),
}

/// A validated permutation and duplication of whole logical value groups.
///
/// Input and output units are numbered from the bottom of the stack. The
/// unit-level constructor makes decoded stack-machine instructions convenient
/// to express while ensuring execution can never split a multi-unit value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShufflePlan {
    input_widths: Vec<usize>,
    output_groups: Vec<usize>,
}

impl ShufflePlan {
    /// Validates a unit-level output layout against whole input value groups.
    ///
    /// A group may be omitted, moved, or repeated. Every occurrence must name
    /// all of its units exactly once and in their original order.
    pub fn new(
        input_widths: impl IntoIterator<Item = usize>,
        output_units: impl IntoIterator<Item = usize>,
    ) -> Result<Self, ShuffleError> {
        let input_widths: Vec<_> = input_widths.into_iter().collect();
        let mut unit_to_group = Vec::new();
        let mut starts = Vec::with_capacity(input_widths.len());
        for (group, &width) in input_widths.iter().enumerate() {
            if width == 0 {
                return Err(ShuffleError::ZeroWidthGroup { group });
            }
            starts.push(unit_to_group.len());
            unit_to_group.extend(std::iter::repeat_n(group, width));
        }

        let output_units: Vec<_> = output_units.into_iter().collect();
        let mut output_groups = Vec::new();
        let mut cursor = 0;
        while cursor < output_units.len() {
            let unit = output_units[cursor];
            let Some(&group) = unit_to_group.get(unit) else {
                return Err(ShuffleError::UnknownUnit { unit });
            };
            let start = starts[group];
            let width = input_widths[group];
            let end = cursor.saturating_add(width);
            let whole_group = output_units
                .get(cursor..end)
                .is_some_and(|units| units.iter().copied().eq(start..start + width));
            if !whole_group {
                return Err(ShuffleError::SplitGroup { group });
            }
            output_groups.push(group);
            cursor = end;
        }

        Ok(Self {
            input_widths,
            output_groups,
        })
    }

    /// Applies the plan atomically, leaving `stack` untouched on every error.
    pub fn execute<P>(&self, stack: &mut UnitStack<P>) -> Result<(), ShuffleError>
    where
        P: ValueWidthPolicy,
        P::Value: Clone,
    {
        let common = self.input_widths.len().min(stack.values.len());
        for group in 0..common {
            let actual = P::width(&stack.values[group]);
            let expected = self.input_widths[group];
            if actual != expected {
                return Err(ShuffleError::LayoutMismatch {
                    group,
                    expected,
                    actual,
                });
            }
        }
        if self.input_widths.len() != stack.values.len() {
            let group = common;
            return Err(ShuffleError::LayoutMismatch {
                group,
                expected: self.input_widths.get(group).copied().unwrap_or(0),
                actual: stack.values.get(group).map(P::width).unwrap_or(0),
            });
        }

        let mut staged = UnitStack::<P>::new(stack.limit);
        for &group in &self.output_groups {
            staged
                .push(stack.values[group].clone())
                .map_err(ShuffleError::Stack)?;
        }
        *stack = staged;
        Ok(())
    }
}
