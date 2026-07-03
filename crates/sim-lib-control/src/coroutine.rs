use sim_kernel::Ref;

/// Identifies which of a coroutine's two cooperating lanes yielded a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoroutineLane {
    /// The first lane.
    First,
    /// The second lane.
    Second,
}

/// Outcome of resuming a [`Coroutine`]: a yielded value, or exhaustion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoroutineStep {
    /// A lane yielded a value and control returned to the driver.
    Yielded {
        /// The lane that produced this value.
        lane: CoroutineLane,
        /// The yielded value.
        value: Ref,
    },
    /// Both lanes are drained; the coroutine has nothing left to yield.
    Exhausted,
}

/// Two cooperating value streams that yield by alternating between lanes.
///
/// Models symmetric coroutine control: each [`Coroutine::resume`] hands control
/// to the next lane, falling through to the other when one is drained.
///
/// # Examples
///
/// ```
/// use sim_kernel::{Ref, Symbol};
/// use sim_lib_control::{Coroutine, CoroutineLane, CoroutineStep};
///
/// let a = Ref::Symbol(Symbol::new("a"));
/// let b = Ref::Symbol(Symbol::new("b"));
/// let mut co = Coroutine::alternating(vec![a.clone()], vec![b.clone()]);
/// assert_eq!(
///     co.resume(),
///     CoroutineStep::Yielded { lane: CoroutineLane::First, value: a }
/// );
/// assert_eq!(
///     co.resume(),
///     CoroutineStep::Yielded { lane: CoroutineLane::Second, value: b }
/// );
/// assert_eq!(co.resume(), CoroutineStep::Exhausted);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coroutine {
    first: Vec<Ref>,
    second: Vec<Ref>,
    first_index: usize,
    second_index: usize,
    next_lane: CoroutineLane,
}

impl Coroutine {
    /// Builds a coroutine that alternates yields between the `first` and
    /// `second` lanes, starting with the first.
    pub fn alternating(first: Vec<Ref>, second: Vec<Ref>) -> Self {
        Self {
            first,
            second,
            first_index: 0,
            second_index: 0,
            next_lane: CoroutineLane::First,
        }
    }

    /// Resumes the coroutine, yielding the next value from the active lane (or
    /// the other lane if the active one is drained), or
    /// [`CoroutineStep::Exhausted`] when both are empty.
    pub fn resume(&mut self) -> CoroutineStep {
        let step = match self.next_lane {
            CoroutineLane::First => self.resume_first().or_else(|| self.resume_second()),
            CoroutineLane::Second => self.resume_second().or_else(|| self.resume_first()),
        };
        step.unwrap_or(CoroutineStep::Exhausted)
    }

    fn resume_first(&mut self) -> Option<CoroutineStep> {
        let value = self.first.get(self.first_index).cloned()?;
        self.first_index += 1;
        self.next_lane = CoroutineLane::Second;
        Some(CoroutineStep::Yielded {
            lane: CoroutineLane::First,
            value,
        })
    }

    fn resume_second(&mut self) -> Option<CoroutineStep> {
        let value = self.second.get(self.second_index).cloned()?;
        self.second_index += 1;
        self.next_lane = CoroutineLane::First;
        Some(CoroutineStep::Yielded {
            lane: CoroutineLane::Second,
            value,
        })
    }
}
