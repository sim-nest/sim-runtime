//! ECMAScript resumable control and explicitly driven reaction jobs.

use std::{cell::RefCell, collections::BTreeMap, fmt, rc::Rc, sync::Arc};

use sim_kernel::{ClassRef, Cx, Object, ObjectCompat, Origin, Result as KernelResult, Symbol};

use sim_lib_control::{
    AdmissionLimit, CheckpointError, CheckpointReceipt, FrameError, FrameLimits, JobId, JobQueues,
    Raised, ResumableFrame, ResumePacket, ResumeResult, RuntimeJobClass, WorkLimit,
};

use crate::JavascriptValue;

#[derive(Debug)]
struct JavascriptThrownFace {
    value: JavascriptValue,
}
impl Object for JavascriptThrownFace {
    fn display(&self, _cx: &mut Cx) -> KernelResult<String> {
        Ok(format!("{:?}", self.value))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for JavascriptThrownFace {}

/// Realm class identities used to classify arbitrary JavaScript thrown values.
///
/// Error subclasses and ordinary objects are registered by managed identity;
/// primitives use only the realm's declared canonical classes.
pub struct JavascriptExceptionRealm {
    undefined_class: ClassRef,
    null_class: ClassRef,
    boolean_class: ClassRef,
    number_class: ClassRef,
    bigint_class: ClassRef,
    string_class: ClassRef,
    object_class: ClassRef,
    managed_classes: BTreeMap<sim_lib_mutation::ManagedId, ClassRef>,
}

impl fmt::Debug for JavascriptExceptionRealm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JavascriptExceptionRealm")
            .field("managed_class_count", &self.managed_classes.len())
            .finish_non_exhaustive()
    }
}

impl JavascriptExceptionRealm {
    /// Declare the canonical primitive and ordinary-object classes for a realm.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        undefined_class: ClassRef,
        null_class: ClassRef,
        boolean_class: ClassRef,
        number_class: ClassRef,
        bigint_class: ClassRef,
        string_class: ClassRef,
        object_class: ClassRef,
    ) -> Self {
        Self {
            undefined_class,
            null_class,
            boolean_class,
            number_class,
            bigint_class,
            string_class,
            object_class,
            managed_classes: BTreeMap::new(),
        }
    }

    /// Associate an ordinary object with its exact realm class (including subclasses).
    pub fn register_managed_class(
        &mut self,
        object: sim_lib_mutation::ManagedHandle,
        class: ClassRef,
    ) {
        self.managed_classes.insert(object.id(), class);
    }

    /// Wrap an arbitrary thrown value in the one shared exceptional-completion envelope.
    pub fn raise(&self, cx: &Cx, value: JavascriptValue, origin: Origin) -> KernelResult<Raised> {
        let class = match &value {
            JavascriptValue::Undefined => self.undefined_class.clone(),
            JavascriptValue::Null => self.null_class.clone(),
            JavascriptValue::Bool(_) => self.boolean_class.clone(),
            JavascriptValue::Number(_) => self.number_class.clone(),
            JavascriptValue::BigInt(_) => self.bigint_class.clone(),
            JavascriptValue::String(_) => self.string_class.clone(),
            JavascriptValue::Managed(handle) => self
                .managed_classes
                .get(&handle.id())
                .cloned()
                .unwrap_or_else(|| self.object_class.clone()),
        };
        let payload = cx
            .factory()
            .opaque(Arc::new(JavascriptThrownFace { value }))?;
        Raised::new(
            class,
            payload,
            origin,
            Symbol::qualified("javascript", "exception"),
        )
    }

    /// Recover the exact JavaScript value retained by this realm's envelope.
    pub fn thrown_value<'a>(&self, raised: &'a Raised) -> Option<&'a JavascriptValue> {
        (raised.profile() == &Symbol::qualified("javascript", "exception"))
            .then(|| {
                raised
                    .payload()
                    .object()
                    .downcast_ref::<JavascriptThrownFace>()
                    .map(|face| &face.value)
            })
            .flatten()
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
    Rejected(Raised),
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
        error: Raised,
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

type JavascriptResumeDriver =
    dyn FnMut(
        ResumePacket<JavascriptValue, Raised>,
        &mut sim_lib_control::StepBudget,
    ) -> Result<ResumeResult<JavascriptValue, JavascriptValue, Raised>, FrameError>;

impl JavascriptGenerator {
    /// Creates a bounded generator from JavaScript policy dispatch.
    pub fn new(
        limits: FrameLimits,
        dispatch: impl FnMut(
            ResumePacket<JavascriptValue, Raised>,
            &mut sim_lib_control::StepBudget,
        ) -> Result<
            ResumeResult<JavascriptValue, JavascriptValue, Raised>,
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
        packet: ResumePacket<JavascriptValue, Raised>,
    ) -> Result<ResumeResult<JavascriptValue, JavascriptValue, Raised>, FrameError> {
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
        packet: ResumePacket<JavascriptValue, Raised>,
        jobs: &mut JavascriptJobs,
    ) -> Result<ResumeResult<JavascriptValue, JavascriptValue, Raised>, FrameError> {
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
    use sim_kernel::{ClassId, CodecId, DefaultFactory, NoopEvalPolicy, SourceId, Span};

    fn cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }

    fn class(cx: &Cx, id: u32, name: &str) -> ClassRef {
        cx.factory()
            .class_stub(ClassId(id), Symbol::qualified("javascript", name))
            .unwrap()
    }

    fn origin(at: usize) -> Origin {
        Origin {
            codec: CodecId(1),
            source: SourceId("exceptions3-javascript".into()),
            span: Span {
                start: at,
                end: at + 1,
            },
            trivia: Default::default(),
        }
    }

    fn realm(cx: &Cx) -> JavascriptExceptionRealm {
        JavascriptExceptionRealm::new(
            class(cx, 1, "Undefined"),
            class(cx, 2, "Null"),
            class(cx, 3, "Boolean"),
            class(cx, 4, "Number"),
            class(cx, 5, "BigInt"),
            class(cx, 6, "String"),
            class(cx, 7, "Object"),
        )
    }

    fn content_id(value: &str) -> u64 {
        value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    }

    fn exception_characterization() -> Vec<String> {
        let cx = cx();
        let realm = realm(&cx);
        let errors = [
            JavascriptValue::Number(3.0),
            JavascriptValue::String("plain throw".into()),
            JavascriptValue::Undefined,
        ];
        let raised = errors
            .iter()
            .cloned()
            .enumerate()
            .map(|(at, value)| realm.raise(&cx, value, origin(at)).unwrap())
            .collect::<Vec<_>>();
        vec![
            format!("number={:?}", realm.thrown_value(&raised[0])),
            format!("string={:?}", realm.thrown_value(&raised[1])),
            format!("undefined={:?}", realm.thrown_value(&raised[2])),
            format!(
                "origins={:?}",
                raised.iter().map(Raised::origin).collect::<Vec<_>>()
            ),
        ]
    }

    #[test]
    fn characterize_1_exception_behavior_replays_identically() {
        let first = exception_characterization();
        let replay = exception_characterization();
        assert_eq!(first, replay);
        assert_eq!(
            first.iter().map(|row| content_id(row)).collect::<Vec<_>>(),
            replay.iter().map(|row| content_id(row)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generator_exception_and_async_settlement_use_shared_packets() {
        let cx = cx();
        let realm = realm(&cx);
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
        let error = realm
            .raise(&cx, JavascriptValue::String("bounded".into()), origin(3))
            .unwrap();
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

        let rejected = JavascriptPromise::default();
        let thrown = realm
            .raise(&cx, JavascriptValue::Number(1.0), origin(4))
            .unwrap();
        rejected.reject(&mut jobs, thrown.clone()).unwrap();
        jobs.microtask_checkpoint(WorkLimit(1)).unwrap();
        assert_eq!(rejected.state(), JavascriptPromiseState::Rejected(thrown));

        let async_error = realm
            .raise(&cx, JavascriptValue::Undefined, origin(5))
            .unwrap();
        let failed = async_error.clone();
        let generator = JavascriptGenerator::new(FrameLimits { depth: 1, work: 1 }, move |_, _| {
            Ok(ResumeResult::Failed(failed.clone()))
        });
        let mut function = JavascriptAsyncFunction::new(generator);
        let promise = function.promise();
        assert_eq!(
            function.resume(ResumePacket::Start, &mut jobs).unwrap(),
            ResumeResult::Failed(async_error.clone())
        );
        jobs.microtask_checkpoint(WorkLimit(1)).unwrap();
        assert_eq!(
            promise.state(),
            JavascriptPromiseState::Rejected(async_error)
        );
    }

    #[test]
    fn arbitrary_throws_keep_values_and_realm_class_identity() {
        let cx = cx();
        let mut realm = realm(&cx);
        let mut heap = crate::JavascriptHeap::retaining(2).unwrap();
        let plain = heap
            .allocate(crate::JavascriptManagedObject::new(
                crate::JavascriptManagedKind::Object,
            ))
            .unwrap();
        let subclassed_error = heap
            .allocate(crate::JavascriptManagedObject::new(
                crate::JavascriptManagedKind::Object,
            ))
            .unwrap();
        let subclass = class(&cx, 8, "DomainError");
        realm.register_managed_class(subclassed_error, subclass.clone());

        for (at, value) in [
            JavascriptValue::Number(1.0),
            JavascriptValue::Undefined,
            JavascriptValue::Managed(plain),
        ]
        .into_iter()
        .enumerate()
        {
            let raised = realm.raise(&cx, value.clone(), origin(at)).unwrap();
            assert_eq!(realm.thrown_value(&raised), Some(&value));
        }
        let raised = realm
            .raise(&cx, JavascriptValue::Managed(subclassed_error), origin(3))
            .unwrap();
        assert_eq!(raised.class_ref(), &subclass);
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
