/// Limits applied to one resumable frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLimits {
    /// Maximum nested frame depth accepted by the driver.
    pub depth: usize,
    /// Maximum work units available to each resume operation.
    pub work: usize,
}

/// Input delivered when a frame is resumed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumePacket<T, E> {
    /// Start a frame that has not run before.
    Start,
    /// Send a value into a suspended frame.
    Send(T),
    /// Throw an error into a suspended frame.
    Throw(E),
    /// Ask a suspended frame to close and run its cleanup.
    Close,
}

/// Observable result of a resume operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumeResult<T, R, E> {
    /// The frame suspended after yielding a value.
    Yielded(T),
    /// The frame completed and returned a value.
    Returned(R),
    /// The frame completed with a failure.
    Failed(E),
}

/// Failure enforced by the frame boundary rather than by its guest driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// A non-start packet was sent before the frame started.
    NotStarted,
    /// Start was sent more than once.
    AlreadyStarted,
    /// A terminal frame was resumed again.
    AlreadyComplete,
    /// The driver exceeded its declared nesting depth.
    DepthExhausted,
    /// The driver exhausted its declared work allowance.
    WorkExhausted,
}

/// Budget passed to a frame driver for one resume operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepBudget {
    depth_left: usize,
    work_left: usize,
}

impl StepBudget {
    /// Charges one work unit, failing closed when none remain.
    pub fn charge_work(&mut self) -> Result<(), FrameError> {
        self.work_left = self
            .work_left
            .checked_sub(1)
            .ok_or(FrameError::WorkExhausted)?;
        Ok(())
    }

    /// Enters one nested frame level, failing closed at the depth limit.
    pub fn enter(&mut self) -> Result<(), FrameError> {
        self.depth_left = self
            .depth_left
            .checked_sub(1)
            .ok_or(FrameError::DepthExhausted)?;
        Ok(())
    }

    /// Leaves a nested frame level.
    pub fn leave(&mut self) {
        self.depth_left = self.depth_left.saturating_add(1);
    }
}

/// A surface-neutral, one-shot-completion resumable frame.
pub struct ResumableFrame<D> {
    driver: D,
    limits: FrameLimits,
    started: bool,
    complete: bool,
}

impl<D> ResumableFrame<D> {
    /// Creates a frame driven by `driver` under explicit limits.
    pub fn new(limits: FrameLimits, driver: D) -> Self {
        Self {
            driver,
            limits,
            started: false,
            complete: false,
        }
    }

    /// Returns whether the frame has returned or failed.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Delivers one packet and returns the next observable transition.
    pub fn resume<T, R, E>(
        &mut self,
        packet: ResumePacket<T, E>,
    ) -> Result<ResumeResult<T, R, E>, FrameError>
    where
        D: FnMut(ResumePacket<T, E>, &mut StepBudget) -> Result<ResumeResult<T, R, E>, FrameError>,
    {
        if self.complete {
            return Err(FrameError::AlreadyComplete);
        }
        match (&packet, self.started) {
            (ResumePacket::Start, true) => return Err(FrameError::AlreadyStarted),
            (ResumePacket::Start, false) => self.started = true,
            (_, false) => return Err(FrameError::NotStarted),
            (_, true) => {}
        }
        let mut budget = StepBudget {
            depth_left: self.limits.depth,
            work_left: self.limits.work,
        };
        let outcome = (self.driver)(packet, &mut budget)?;
        self.complete = !matches!(outcome, ResumeResult::Yielded(_));
        Ok(outcome)
    }
}
