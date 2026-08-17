//! Lossless adaptation of kernel call arguments for guest policy.

use sim_kernel::{Args, Symbol, Value};

/// The exact place at which an argument entered the call boundary.
///
/// Origins are data rather than formatted diagnostics so a guest can retain
/// its own source vocabulary while still identifying every occurrence.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ArgumentOrigin {
    /// A zero-based position in the kernel's evaluated [`Args`] sequence.
    KernelPosition(usize),
    /// A guest adapter's stable source location or expansion identity.
    Guest(Symbol),
}

/// One undecided input supplied to the neutral adaptation boundary.
///
/// The variants describe how an upstream adapter observed the value. They do
/// not imply precedence, legality, parameter assignment, or default behavior.
#[derive(Clone)]
pub enum ArgumentInput {
    /// An ordinary positional input.
    Positional(Value),
    /// One named occurrence. Duplicate names remain distinct occurrences.
    Named {
        /// The caller-supplied name for this occurrence.
        name: Symbol,
        /// The caller-supplied value for this occurrence.
        value: Value,
    },
    /// A call-site receiver offered for guest-policy interpretation.
    Receiver(Value),
    /// An already-expanded remainder input offered by an upstream adapter.
    Remainder(Value),
    /// An input an upstream adapter could not classify.
    Unconsumed(Value),
}

/// An input paired with its exact origin.
#[derive(Clone)]
pub struct BoundArgument {
    input: ArgumentInput,
    origin: ArgumentOrigin,
}

impl BoundArgument {
    /// Creates one explicitly originated input.
    pub fn new(input: ArgumentInput, origin: ArgumentOrigin) -> Self {
        Self { input, origin }
    }

    /// Returns the undecided input classification.
    pub const fn input(&self) -> &ArgumentInput {
        &self.input
    }

    /// Returns the exact input origin.
    pub const fn origin(&self) -> &ArgumentOrigin {
        &self.origin
    }
}

/// Mutable call input assembled by kernel and guest adapters.
#[derive(Clone, Default)]
pub struct CallInput {
    arguments: Vec<BoundArgument>,
}

impl CallInput {
    /// Starts an empty adaptation stream.
    pub const fn new() -> Self {
        Self {
            arguments: Vec::new(),
        }
    }

    /// Appends an input without interpreting any earlier occurrence.
    pub fn push(&mut self, input: ArgumentInput, origin: ArgumentOrigin) {
        self.arguments.push(BoundArgument::new(input, origin));
    }

    /// Appends an input and returns the stream for fluent adapter construction.
    pub fn with(mut self, input: ArgumentInput, origin: ArgumentOrigin) -> Self {
        self.push(input, origin);
        self
    }

    /// Returns the complete input stream in arrival order.
    pub fn arguments(&self) -> &[BoundArgument] {
        &self.arguments
    }
}

impl From<Args> for CallInput {
    fn from(args: Args) -> Self {
        Self {
            arguments: args
                .into_vec()
                .into_iter()
                .enumerate()
                .map(|(position, value)| {
                    BoundArgument::new(
                        ArgumentInput::Positional(value),
                        ArgumentOrigin::KernelPosition(position),
                    )
                })
                .collect(),
        }
    }
}

/// A stable, lossless call record awaiting guest-policy decisions.
#[derive(Clone, Default)]
pub struct BoundCall {
    arguments: Vec<BoundArgument>,
}

impl BoundCall {
    /// Returns every input in original arrival order.
    pub fn arguments(&self) -> &[BoundArgument] {
        &self.arguments
    }

    /// Returns all positional occurrences in arrival order.
    pub fn positional(&self) -> impl Iterator<Item = &BoundArgument> {
        self.select(|input| matches!(input, ArgumentInput::Positional(_)))
    }

    /// Returns all named occurrences in arrival order, including duplicates.
    pub fn named(&self) -> impl Iterator<Item = &BoundArgument> {
        self.select(|input| matches!(input, ArgumentInput::Named { .. }))
    }

    /// Returns all receiver occurrences in arrival order.
    pub fn receivers(&self) -> impl Iterator<Item = &BoundArgument> {
        self.select(|input| matches!(input, ArgumentInput::Receiver(_)))
    }

    /// Returns all remainder occurrences in arrival order.
    pub fn remainder(&self) -> impl Iterator<Item = &BoundArgument> {
        self.select(|input| matches!(input, ArgumentInput::Remainder(_)))
    }

    /// Returns all unconsumed occurrences in arrival order.
    pub fn unconsumed(&self) -> impl Iterator<Item = &BoundArgument> {
        self.select(|input| matches!(input, ArgumentInput::Unconsumed(_)))
    }

    fn select(
        &self,
        predicate: impl Fn(&ArgumentInput) -> bool,
    ) -> impl Iterator<Item = &BoundArgument> {
        self.arguments
            .iter()
            .filter(move |argument| predicate(&argument.input))
    }
}

/// Freezes an assembled input stream without applying a language rule.
pub fn bind(input: CallInput) -> BoundCall {
    BoundCall {
        arguments: input.arguments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::testing::bare_cx;

    fn origin(name: &str) -> ArgumentOrigin {
        ArgumentOrigin::Guest(Symbol::new(name))
    }

    #[test]
    fn duplicate_names_reach_policy_as_distinct_ordered_occurrences() {
        let cx = bare_cx();
        let first = cx.factory().symbol(Symbol::new("first")).unwrap();
        let second = cx.factory().symbol(Symbol::new("second")).unwrap();
        let name = Symbol::new("option");
        let input = CallInput::new()
            .with(
                ArgumentInput::Named {
                    name: name.clone(),
                    value: first,
                },
                origin("call:4"),
            )
            .with(
                ArgumentInput::Named {
                    name,
                    value: second,
                },
                origin("call:9"),
            );

        let bound = bind(input);
        let origins = bound.named().map(BoundArgument::origin).collect::<Vec<_>>();
        assert_eq!(origins, vec![&origin("call:4"), &origin("call:9")]);
    }

    #[test]
    fn every_input_class_remains_visible_and_stably_ordered() {
        let cx = bare_cx();
        let make = || {
            let value = |name| cx.factory().symbol(Symbol::new(name)).unwrap();
            let positional = value("positional");
            let receiver = value("receiver");
            let remainder = value("remainder");
            let unconsumed = value("unconsumed");
            CallInput::from(Args::new(vec![positional]))
                .with(ArgumentInput::Receiver(receiver), origin("receiver"))
                .with(ArgumentInput::Remainder(remainder), origin("spread"))
                .with(ArgumentInput::Unconsumed(unconsumed), origin("unknown"))
        };

        let project = |bound: BoundCall| {
            bound
                .arguments()
                .iter()
                .map(|argument| argument.origin().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(project(bind(make())), project(bind(make())));
        let bound = bind(make());
        assert_eq!(bound.positional().count(), 1);
        assert_eq!(bound.receivers().count(), 1);
        assert_eq!(bound.remainder().count(), 1);
        assert_eq!(bound.unconsumed().count(), 1);
    }
}
