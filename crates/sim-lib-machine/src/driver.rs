use sim_kernel::ContentId;
use sim_lib_control::{CleanupStack, Unwind, WorkLimit};

use crate::{
    AdmissionPolicy, CodeCursor, Frame, FrameStack, FrameStackError, InstructionPolicy,
    MachineDescription, MachinePermit, SourceLocation, ValueWidthPolicy,
};

/// The cursor access needed by the neutral driver.
///
/// Consumers may use [`Frame`] or provide a distinct activation record.
pub trait MachineFrame {
    /// Returns the instruction to execute next.
    fn cursor(&self) -> CodeCursor;

    /// Selects the next validated instruction boundary.
    fn set_cursor(&mut self, cursor: CodeCursor);
}

impl<P, K, R, H> MachineFrame for Frame<P, K, R, H>
where
    P: ValueWidthPolicy,
{
    fn cursor(&self) -> CodeCursor {
        self.cursor()
    }

    fn set_cursor(&mut self, cursor: CodeCursor) {
        self.set_cursor(cursor);
    }
}

/// The explicit result of executing exactly one decoded instruction.
pub enum StepOutcome<F, R, A, Y, I> {
    /// Continue in the current frame at the supplied validated cursor.
    Continue(CodeCursor),
    /// Push a new guest frame. This is data interpreted by the loop, not a Rust call.
    Call(F),
    /// Pop the current guest frame and carry consumer-defined return data.
    Return(R),
    /// Stop with a consumer-defined abrupt outcome.
    Raise(A),
    /// Cooperatively yield consumer-defined state.
    Yield(Y),
    /// Stop for a consumer-defined interrupt.
    Interrupt(I),
}

/// Result type returned by one consumer instruction policy invocation.
pub type PolicyStep<F, R, A, Y, I, E> = Result<StepOutcome<F, R, A, Y, I>, E>;

/// Stable classification recorded for each charged instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// Ordinary continuation.
    Continue,
    /// Guest-frame push.
    Call,
    /// Guest-frame pop.
    Return,
    /// Abrupt outcome.
    Raise,
    /// Cooperative yield.
    Yield,
    /// Interrupt.
    Interrupt,
}

/// Deterministic evidence for the exact instruction work performed by one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkReceipt<Id> {
    steps: Vec<(Id, StepKind)>,
}

impl<Id> WorkReceipt<Id> {
    /// Returns the exact amount of charged instruction work.
    pub fn charged(&self) -> usize {
        self.steps.len()
    }

    /// Returns ordered instruction identities and their control outcomes.
    pub fn steps(&self) -> &[(Id, StepKind)] {
        &self.steps
    }

    /// Appends later work to this receipt prefix without changing its order.
    pub fn append(&mut self, later: Self) {
        self.steps.extend(later.steps);
    }
}

/// Abrupt terminal reasons carried through the control organ's unwind vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineAbrupt<A, I> {
    /// No prepared protected region handled a raised value.
    Raise(A),
    /// Execution stopped at an interruption checkpoint.
    Interrupt(I),
    /// The bounded drive consumed its complete allowance.
    BudgetExhausted,
    /// Admission, frame, or instruction execution failed closed.
    Fault,
}

/// Control-organ reason delivered exactly once to registered machine cleanups.
pub type MachineUnwind<R, A, I> = Unwind<R, (), (), MachineAbrupt<A, I>>;

/// Content-bound evidence for a suspended continuation and its receipt prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationEvidence<Id> {
    content_id: ContentId,
    receipt: WorkReceipt<Id>,
}

impl<Id> ContinuationEvidence<Id> {
    /// Returns the admitted program identity to which this continuation belongs.
    pub fn content_id(&self) -> &ContentId {
        &self.content_id
    }

    /// Returns all work observed before suspension.
    pub fn receipt(&self) -> &WorkReceipt<Id> {
        &self.receipt
    }
}

/// Owned resumable machine state; construction requires an admitted permit.
#[derive(Debug)]
pub struct MachineCheckpoint<F, Id> {
    frames: FrameStack<F>,
    evidence: ContinuationEvidence<Id>,
}

impl<F, Id> MachineCheckpoint<F, Id> {
    /// Binds suspended frames and their ordered receipt prefix to admitted content.
    pub fn new(frames: FrameStack<F>, permit: &MachinePermit, receipt: WorkReceipt<Id>) -> Self {
        Self {
            frames,
            evidence: ContinuationEvidence {
                content_id: permit.content_id().clone(),
                receipt,
            },
        }
    }

    /// Returns the immutable continuation evidence.
    pub fn evidence(&self) -> &ContinuationEvidence<Id> {
        &self.evidence
    }

    /// Resumes only when the supplied permit admits the checkpoint's exact content.
    pub fn resume(self, permit: &MachinePermit) -> Result<(FrameStack<F>, WorkReceipt<Id>), Self> {
        if self.evidence.content_id == *permit.content_id() {
            Ok((self.frames, self.evidence.receipt))
        } else {
            Err(self)
        }
    }
}

/// Consumer semantics invoked once per charged instruction.
///
/// A WebAssembly engine or a bytecode-independent workflow machine can supply
/// this policy. The callback cannot run until admission identity is checked.
pub trait InstructionDriverPolicy<P, F>
where
    P: InstructionPolicy,
{
    /// Return transfer data.
    type Return;
    /// Abrupt outcome.
    type Abrupt;
    /// Cooperative yield data.
    type Yield;
    /// Interrupt data.
    type Interrupt;
    /// Instruction failure.
    type Fault;

    /// Executes one instruction policy and returns an explicit control outcome.
    #[allow(
        clippy::type_complexity,
        reason = "the explicit outcome channels are the driver contract"
    )]
    fn step(
        &mut self,
        instruction: &P::Instruction,
        frame: &mut F,
    ) -> PolicyStep<F, Self::Return, Self::Abrupt, Self::Yield, Self::Interrupt, Self::Fault>;
}

/// An instruction failure paired with its stable identity and source location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedFault<Id, E> {
    /// Stable identity of the failing instruction.
    pub instruction: Id,
    /// Exact prepared source location of the failing instruction.
    pub location: SourceLocation,
    /// Consumer-defined failure.
    pub fault: E,
}

/// A completed or suspended iterative drive.
pub enum DriveOutcome<Id, R, A, Y, I> {
    /// Work was exhausted with explicit guest frames still runnable.
    Continue(WorkReceipt<Id>),
    /// The outermost guest frame returned.
    Return(R, WorkReceipt<Id>),
    /// An instruction raised an abrupt outcome.
    Raise(A, WorkReceipt<Id>),
    /// An instruction cooperatively yielded.
    Yield(Y, WorkReceipt<Id>),
    /// An instruction requested interruption.
    Interrupt(I, WorkReceipt<Id>),
}

/// A refusal produced by the iterative driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriveError<Id, E> {
    /// The permit does not bind the exact supplied description.
    PermitMismatch,
    /// No frame exists to execute.
    EmptyFrames,
    /// A guest call exceeded the explicit frame budget.
    FrameLimit(FrameStackError),
    /// Instruction policy failed at a prepared source location.
    Fault(LocatedFault<Id, E>),
}

/// Result type returned by one bounded drive operation.
pub type DriveResult<Id, R, A, Y, I, E> = Result<DriveOutcome<Id, R, A, Y, I>, DriveError<Id, E>>;

/// Bounded iterative instruction driver.
pub struct Driver<D> {
    policy: D,
}

impl<D> Driver<D> {
    /// Creates a driver around consumer-owned instruction semantics.
    pub fn new(policy: D) -> Self {
        Self { policy }
    }

    /// Drives at most `work` instructions without recursive guest execution.
    #[allow(
        clippy::type_complexity,
        reason = "the explicit outcome channels are the driver contract"
    )]
    pub fn drive<P, M, A, F>(
        &mut self,
        description: &MachineDescription<'_, P, M>,
        permit: &MachinePermit,
        frames: &mut FrameStack<F>,
        work: WorkLimit,
    ) -> DriveResult<P::InstructionId, D::Return, D::Abrupt, D::Yield, D::Interrupt, D::Fault>
    where
        P: InstructionPolicy,
        P::InstructionId: Copy + Eq + Ord,
        A: AdmissionPolicy<P, M>,
        F: MachineFrame,
        D: InstructionDriverPolicy<P, F>,
    {
        // This identity check deliberately precedes every access to `self.policy`.
        if !permit.accepts::<P, M, A>(description) {
            return Err(DriveError::PermitMismatch);
        }
        if frames.current().is_none() {
            return Err(DriveError::EmptyFrames);
        }

        let mut steps = Vec::with_capacity(work.0);
        for _ in 0..work.0 {
            let cursor = frames.current().ok_or(DriveError::EmptyFrames)?.cursor();
            let located = description.code().instruction(cursor);
            let id = *located.id();
            let location = located.location().clone();
            let outcome = self
                .policy
                .step(
                    located.instruction(),
                    frames.current_mut().expect("frame was checked"),
                )
                .map_err(|fault| {
                    DriveError::Fault(LocatedFault {
                        instruction: id,
                        location,
                        fault,
                    })
                })?;
            match outcome {
                StepOutcome::Continue(next) => {
                    steps.push((id, StepKind::Continue));
                    frames
                        .current_mut()
                        .expect("current frame remains")
                        .set_cursor(next);
                }
                StepOutcome::Call(frame) => {
                    steps.push((id, StepKind::Call));
                    frames.push(frame).map_err(DriveError::FrameLimit)?;
                }
                StepOutcome::Return(value) => {
                    steps.push((id, StepKind::Return));
                    frames.pop();
                    if frames.current().is_none() {
                        return Ok(DriveOutcome::Return(value, WorkReceipt { steps }));
                    }
                }
                StepOutcome::Raise(value) => {
                    steps.push((id, StepKind::Raise));
                    return Ok(DriveOutcome::Raise(value, WorkReceipt { steps }));
                }
                StepOutcome::Yield(value) => {
                    steps.push((id, StepKind::Yield));
                    return Ok(DriveOutcome::Yield(value, WorkReceipt { steps }));
                }
                StepOutcome::Interrupt(value) => {
                    steps.push((id, StepKind::Interrupt));
                    return Ok(DriveOutcome::Interrupt(value, WorkReceipt { steps }));
                }
            }
        }
        Ok(DriveOutcome::Continue(WorkReceipt { steps }))
    }

    /// Drives through prepared protected regions, selecting the innermost handler.
    ///
    /// A handled raise remains visible in the receipt, then execution continues at
    /// the validated handler boundary. The abrupt value remains consumer-owned;
    /// policies place it in their frame's handler state before returning `Raise`.
    #[allow(
        clippy::type_complexity,
        reason = "matches the explicit driver channels"
    )]
    pub fn drive_protected<P, M, A, F>(
        &mut self,
        description: &MachineDescription<'_, P, M>,
        permit: &MachinePermit,
        frames: &mut FrameStack<F>,
        work: WorkLimit,
    ) -> DriveResult<P::InstructionId, D::Return, D::Abrupt, D::Yield, D::Interrupt, D::Fault>
    where
        P: InstructionPolicy,
        P::InstructionId: Copy + Eq + Ord,
        A: AdmissionPolicy<P, M>,
        F: MachineFrame,
        D: InstructionDriverPolicy<P, F>,
    {
        let mut combined = WorkReceipt { steps: Vec::new() };
        let mut left = work.0;
        while left > 0 {
            let cursor = frames.current().ok_or(DriveError::EmptyFrames)?.cursor();
            let outcome = self.drive::<P, M, A, F>(description, permit, frames, WorkLimit(1))?;
            match outcome {
                DriveOutcome::Raise(value, receipt) => {
                    left -= receipt.charged();
                    combined.append(receipt);
                    if let Some(region) = description.code().innermost_protected_region(cursor) {
                        frames
                            .current_mut()
                            .expect("raising frame remains")
                            .set_cursor(region.handler);
                    } else {
                        return Ok(DriveOutcome::Raise(value, combined));
                    }
                }
                DriveOutcome::Continue(receipt) => {
                    left -= receipt.charged();
                    combined.append(receipt);
                    if left == 0 {
                        return Ok(DriveOutcome::Continue(combined));
                    }
                }
                DriveOutcome::Return(value, receipt) => {
                    combined.append(receipt);
                    return Ok(DriveOutcome::Return(value, combined));
                }
                DriveOutcome::Yield(value, receipt) => {
                    combined.append(receipt);
                    return Ok(DriveOutcome::Yield(value, combined));
                }
                DriveOutcome::Interrupt(value, receipt) => {
                    combined.append(receipt);
                    return Ok(DriveOutcome::Interrupt(value, combined));
                }
            }
        }
        Ok(DriveOutcome::Continue(combined))
    }

    /// Runs a protected drive and unwinds registered cleanups on every abrupt or
    /// terminal path. Yield is a suspension and therefore retains its dynamic extent.
    #[allow(
        clippy::type_complexity,
        reason = "matches the explicit driver channels"
    )]
    pub fn drive_with_cleanup<P, M, A, F>(
        &mut self,
        description: &MachineDescription<'_, P, M>,
        permit: &MachinePermit,
        frames: &mut FrameStack<F>,
        work: WorkLimit,
        cleanups: CleanupStack<MachineUnwind<D::Return, D::Abrupt, D::Interrupt>>,
    ) -> DriveResult<P::InstructionId, D::Return, D::Abrupt, D::Yield, D::Interrupt, D::Fault>
    where
        P: InstructionPolicy,
        P::InstructionId: Copy + Eq + Ord,
        A: AdmissionPolicy<P, M>,
        F: MachineFrame,
        D: InstructionDriverPolicy<P, F>,
        D::Return: Clone,
        D::Abrupt: Clone,
        D::Interrupt: Clone,
    {
        let outcome = match self.drive_protected::<P, M, A, F>(description, permit, frames, work) {
            Ok(outcome) => outcome,
            Err(error) => {
                cleanups.unwind(Unwind::Exception(MachineAbrupt::Fault));
                return Err(error);
            }
        };
        match &outcome {
            DriveOutcome::Return(value, _) => {
                cleanups.unwind(Unwind::Return(value.clone()));
            }
            DriveOutcome::Raise(value, _) => {
                cleanups.unwind(Unwind::Exception(MachineAbrupt::Raise(value.clone())));
            }
            DriveOutcome::Interrupt(value, _) => {
                cleanups.unwind(Unwind::Exception(MachineAbrupt::Interrupt(value.clone())));
            }
            DriveOutcome::Continue(_) => {
                cleanups.unwind(Unwind::Exception(MachineAbrupt::BudgetExhausted));
            }
            DriveOutcome::Yield(_, _) => return Ok(outcome),
        }
        Ok(outcome)
    }
}
