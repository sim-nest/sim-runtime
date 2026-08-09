//! ECMAScript resumable control and explicitly driven reaction jobs.

use std::{cell::RefCell, rc::Rc};

use sim_lib_control::{
    AdmissionLimit, CheckpointError, CheckpointReceipt, FrameError, FrameLimits, JobId, JobQueues,
    ResumableFrame, ResumePacket, ResumeResult, RuntimeJobClass, WorkLimit,
};

use crate::JavascriptValue;

/// JavaScript exception value carried by shared resume packets.
#[derive(Clone, Debug, PartialEq)]
pub struct JavascriptException {
    /// Stable exception name such as `TypeError`.
    pub name: String,
    /// Human-readable exception message.
    pub message: String,
}

impl JavascriptException {
    /// Creates an exception without consulting ambient host state.
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
        }
    }
}

/// Queue classes used by the JavaScript profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JavascriptJobClass {
    /// Promise reactions, async continuations, and dynamic-import evaluation.
    Microtask,
    /// Collector-admitted finalization work, never drained by a microtask checkpoint.
    Finalization,
}

impl From<JavascriptJobClass> for RuntimeJobClass {
    fn from(value: JavascriptJobClass) -> Self {
        match value {
            JavascriptJobClass::Microtask => RuntimeJobClass::LanguageMicrotask("javascript"),
            JavascriptJobClass::Finalization => RuntimeJobClass::Finalization,
        }
    }
}

/// Explicit JavaScript job organ. It owns no thread, timer, or host event loop.
pub struct JavascriptJobs {
    queues: JobQueues<RuntimeJobClass>,
}

impl JavascriptJobs {
    /// Creates queues under a lifetime admission limit.
    pub fn new(admission: AdmissionLimit) -> Self {
        Self {
            queues: JobQueues::new(admission),
        }
    }

    /// Admits a promise/module reaction into the JavaScript microtask FIFO.
    pub fn enqueue_microtask(
        &mut self,
        job: impl FnOnce(&mut JobQueues<RuntimeJobClass>) + 'static,
    ) -> Result<JobId, CheckpointError> {
        self.queues
            .enqueue(RuntimeJobClass::LanguageMicrotask("javascript"), job)
            .map(|receipt| receipt.id)
    }

    /// Admits collector finalization independently of JavaScript microtasks.
    pub fn enqueue_finalization(
        &mut self,
        job: impl FnOnce(&mut JobQueues<RuntimeJobClass>) + 'static,
    ) -> Result<JobId, CheckpointError> {
        self.queues
            .enqueue(RuntimeJobClass::Finalization, job)
            .map(|receipt| receipt.id)
    }

    /// Cancels queued work before an explicit checkpoint.
    pub fn cancel(&mut self, id: JobId) {
        self.queues.cancel(id);
    }

    /// Drains the JavaScript microtask class to empty, including reentrant reactions.
    pub fn microtask_checkpoint(
        &mut self,
        work: WorkLimit,
    ) -> Result<CheckpointReceipt<RuntimeJobClass>, CheckpointError> {
        self.queues
            .checkpoint(RuntimeJobClass::LanguageMicrotask("javascript"), work)
    }

    /// Explicitly drains collector finalization without touching microtasks.
    pub fn finalization_checkpoint(
        &mut self,
        work: WorkLimit,
    ) -> Result<CheckpointReceipt<RuntimeJobClass>, CheckpointError> {
        self.queues.checkpoint(RuntimeJobClass::Finalization, work)
    }
}

/// Promise state observed through a stable shared cell.
#[derive(Clone, Debug, PartialEq)]
pub enum JavascriptPromiseState {
    /// No settlement has occurred.
    Pending,
    /// Fulfilled with one value.
    Fulfilled(JavascriptValue),
    /// Rejected with one exception.
    Rejected(JavascriptException),
}

/// A promise whose reactions are admitted only to explicit JavaScript jobs.
#[derive(Clone)]
pub struct JavascriptPromise(Rc<RefCell<JavascriptPromiseState>>);

impl Default for JavascriptPromise {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(JavascriptPromiseState::Pending)))
    }
}

impl JavascriptPromise {
    /// Snapshots the current settlement state.
    pub fn state(&self) -> JavascriptPromiseState {
        self.0.borrow().clone()
    }

    /// Enqueues first-settlement fulfillment as a reaction job.
    pub fn resolve(
        &self,
        jobs: &mut JavascriptJobs,
        value: JavascriptValue,
    ) -> Result<JobId, CheckpointError> {
        let state = Rc::clone(&self.0);
        jobs.enqueue_microtask(move |_| {
            let mut state = state.borrow_mut();
            if matches!(*state, JavascriptPromiseState::Pending) {
                *state = JavascriptPromiseState::Fulfilled(value);
            }
        })
    }

    /// Enqueues first-settlement rejection as a reaction job.
    pub fn reject(
        &self,
        jobs: &mut JavascriptJobs,
        error: JavascriptException,
    ) -> Result<JobId, CheckpointError> {
        let state = Rc::clone(&self.0);
        jobs.enqueue_microtask(move |_| {
            let mut state = state.borrow_mut();
            if matches!(*state, JavascriptPromiseState::Pending) {
                *state = JavascriptPromiseState::Rejected(error);
            }
        })
    }
}

/// Generator composed from the shared bounded resumable frame.
pub struct JavascriptGenerator {
    frame: ResumableFrame<Box<JavascriptResumeDriver>>,
}

type JavascriptResumeDriver = dyn FnMut(
    ResumePacket<JavascriptValue, JavascriptException>,
    &mut sim_lib_control::StepBudget,
) -> Result<
    ResumeResult<JavascriptValue, JavascriptValue, JavascriptException>,
    FrameError,
>;

impl JavascriptGenerator {
    /// Creates a bounded generator from JavaScript policy dispatch.
    pub fn new(
        limits: FrameLimits,
        dispatch: impl FnMut(
            ResumePacket<JavascriptValue, JavascriptException>,
            &mut sim_lib_control::StepBudget,
        ) -> Result<
            ResumeResult<JavascriptValue, JavascriptValue, JavascriptException>,
            FrameError,
        > + 'static,
    ) -> Self {
        Self {
            frame: ResumableFrame::new(limits, Box::new(dispatch) as Box<JavascriptResumeDriver>),
        }
    }

    /// Resumes with `next`, `throw`, or `return` represented by a shared packet.
    pub fn resume(
        &mut self,
        packet: ResumePacket<JavascriptValue, JavascriptException>,
    ) -> Result<ResumeResult<JavascriptValue, JavascriptValue, JavascriptException>, FrameError>
    {
        self.frame.resume(packet)
    }
}

/// Async function execution is a generator whose terminal result settles a promise.
pub struct JavascriptAsyncFunction {
    generator: JavascriptGenerator,
    promise: JavascriptPromise,
}

impl JavascriptAsyncFunction {
    /// Wraps a resumable body and its externally visible promise.
    pub fn new(generator: JavascriptGenerator) -> Self {
        Self {
            generator,
            promise: JavascriptPromise::default(),
        }
    }

    /// Promise settled by explicit calls to [`Self::resume`].
    pub fn promise(&self) -> JavascriptPromise {
        self.promise.clone()
    }

    /// Advances the body and admits terminal settlement to the microtask queue.
    pub fn resume(
        &mut self,
        packet: ResumePacket<JavascriptValue, JavascriptException>,
        jobs: &mut JavascriptJobs,
    ) -> Result<ResumeResult<JavascriptValue, JavascriptValue, JavascriptException>, FrameError>
    {
        let result = self.generator.resume(packet)?;
        match &result {
            ResumeResult::Returned(value) => {
                self.promise
                    .resolve(jobs, value.clone())
                    .map_err(|_| FrameError::WorkExhausted)?;
            }
            ResumeResult::Failed(error) => {
                self.promise
                    .reject(jobs, error.clone())
                    .map_err(|_| FrameError::WorkExhausted)?;
            }
            ResumeResult::Yielded(_) => {}
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn generator_exception_and_async_settlement_use_shared_packets() {
        let mut generator =
            JavascriptGenerator::new(FrameLimits { depth: 2, work: 4 }, |packet, budget| {
                budget.charge_work()?;
                Ok(match packet {
                    ResumePacket::Start => ResumeResult::Yielded(JavascriptValue::Number(1.0)),
                    ResumePacket::Send(value) => ResumeResult::Returned(value),
                    ResumePacket::Throw(error) => ResumeResult::Failed(error),
                    ResumePacket::Close => ResumeResult::Returned(JavascriptValue::Undefined),
                })
            });
        assert!(matches!(
            generator.resume(ResumePacket::Start).unwrap(),
            ResumeResult::Yielded(JavascriptValue::Number(1.0))
        ));
        let error = JavascriptException::new("RangeError", "bounded");
        assert_eq!(
            generator
                .resume(ResumePacket::Throw(error.clone()))
                .unwrap(),
            ResumeResult::Failed(error)
        );

        let generator = JavascriptGenerator::new(FrameLimits { depth: 1, work: 1 }, |_, _| {
            Ok(ResumeResult::Returned(JavascriptValue::Number(42.0)))
        });
        let mut function = JavascriptAsyncFunction::new(generator);
        let promise = function.promise();
        let mut jobs = JavascriptJobs::new(AdmissionLimit(4));
        function.resume(ResumePacket::Start, &mut jobs).unwrap();
        assert_eq!(promise.state(), JavascriptPromiseState::Pending);
        jobs.microtask_checkpoint(WorkLimit(4)).unwrap();
        assert_eq!(
            promise.state(),
            JavascriptPromiseState::Fulfilled(JavascriptValue::Number(42.0))
        );
    }

    #[test]
    fn checkpoint_drains_reentrant_microtasks_and_isolates_finalization() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut jobs = JavascriptJobs::new(AdmissionLimit(8));
        let outer_trace = Rc::clone(&trace);
        jobs.enqueue_microtask(move |queues| {
            outer_trace.borrow_mut().push("reaction-1");
            let inner_trace = Rc::clone(&outer_trace);
            queues
                .enqueue(
                    RuntimeJobClass::LanguageMicrotask("javascript"),
                    move |_| {
                        inner_trace.borrow_mut().push("reaction-2");
                    },
                )
                .unwrap();
        })
        .unwrap();
        let final_trace = Rc::clone(&trace);
        jobs.enqueue_finalization(move |_| final_trace.borrow_mut().push("finalize"))
            .unwrap();
        let receipt = jobs.microtask_checkpoint(WorkLimit(3)).unwrap();
        assert_eq!(receipt.completed.len(), 2);
        assert_eq!(&*trace.borrow(), &["reaction-1", "reaction-2"]);
        jobs.finalization_checkpoint(WorkLimit(1)).unwrap();
        assert_eq!(&*trace.borrow(), &["reaction-1", "reaction-2", "finalize"]);
    }

    #[test]
    fn exhaustion_and_cancellation_fail_closed_without_implicit_work() {
        let ran = Rc::new(RefCell::new(0));
        let mut jobs = JavascriptJobs::new(AdmissionLimit(2));
        let first_ran = Rc::clone(&ran);
        let first = jobs
            .enqueue_microtask(move |_| *first_ran.borrow_mut() += 1)
            .unwrap();
        let second_ran = Rc::clone(&ran);
        jobs.enqueue_microtask(move |_| *second_ran.borrow_mut() += 1)
            .unwrap();
        assert_eq!(*ran.borrow(), 0);
        assert_eq!(
            jobs.enqueue_microtask(|_| {}).unwrap_err(),
            CheckpointError::AdmissionExhausted
        );
        jobs.cancel(first);
        assert_eq!(
            jobs.microtask_checkpoint(WorkLimit(1)).unwrap_err(),
            CheckpointError::WorkExhausted
        );
        assert_eq!(*ran.borrow(), 0);
        jobs.microtask_checkpoint(WorkLimit(1)).unwrap();
        assert_eq!(*ran.borrow(), 1);
    }
}
