use sim_kernel::Ref;

/// Outcome of advancing a [`Generator`]: a yielded value, or exhaustion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneratorStep {
    /// The next value the generator produces.
    Yielded(Ref),
    /// The generator has yielded every value and is drained.
    Exhausted,
}

/// A single-lane generator that yields a fixed sequence of values on demand.
///
/// Models the yield-to-driver surface of generator control: each
/// [`Generator::next_step`] resumes the generator and hands back the next value.
///
/// # Examples
///
/// ```
/// use sim_kernel::{Ref, Symbol};
/// use sim_lib_control::{Generator, GeneratorStep};
///
/// let one = Ref::Symbol(Symbol::new("one"));
/// let mut generator = Generator::new(vec![one.clone()]);
/// assert_eq!(generator.next_step(), GeneratorStep::Yielded(one));
/// assert_eq!(generator.next_step(), GeneratorStep::Exhausted);
/// assert!(generator.is_exhausted());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generator {
    values: Vec<Ref>,
    index: usize,
}

impl Generator {
    /// Builds a generator that yields `values` in order.
    pub fn new(values: Vec<Ref>) -> Self {
        Self { values, index: 0 }
    }

    /// Advances the generator, returning the next [`GeneratorStep::Yielded`]
    /// value or [`GeneratorStep::Exhausted`] once drained.
    pub fn next_step(&mut self) -> GeneratorStep {
        let Some(value) = self.values.get(self.index).cloned() else {
            return GeneratorStep::Exhausted;
        };
        self.index += 1;
        GeneratorStep::Yielded(value)
    }

    /// Returns whether every value has been yielded.
    pub fn is_exhausted(&self) -> bool {
        self.index >= self.values.len()
    }
}
