//! Python exception and resumable-control policy over the shared control organ.

use std::{fmt, sync::Arc};

use sim_kernel::{
    ClassId, ClassRef, Cx, Object, ObjectCompat, Origin, Result as KernelResult, Symbol,
};
use sim_lib_control::{
    BoundedSubclassOutcome, ClassMatchBudget, ClassMatchEvidence, ClassMatchOutcome, FrameError,
    FrameLimits, ManagedException, Raised, ResumableFrame, ResumePacket, ResumeResult,
    match_raised_class,
};
use sim_lib_mutation::{
    ArenaError, HardCappedRetainPolicy, ManagedArena, ManagedHandle, StrongEdgeMutationError,
};

use crate::PythonObjectSpace;

/// Checked Python iterator state over an owned sequence.
pub struct PythonIterator<T> {
    values: std::vec::IntoIter<T>,
}
impl<T> PythonIterator<T> {
    /// Construct an iterator whose exhaustion is explicit and stable.
    pub fn new(values: Vec<T>) -> Self {
        Self {
            values: values.into_iter(),
        }
    }
    /// Return the next value or `None` for Python `StopIteration`.
    pub fn next_checked(&mut self) -> Option<T> {
        self.values.next()
    }
}

/// Python-owned meaning of one managed exception edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PythonExceptionRelation {
    /// Explicit `raise ... from ...` relation.
    Cause,
    /// Implicit active-exception relation.
    Context,
    /// Ordered direct member of an exception group.
    GroupMember(usize),
}

/// Non-recursive Python data stored in a shared managed exception node.
#[derive(Clone, Debug)]
pub struct PythonExceptionData {
    class: ClassRef,
    message: String,
    origin: Origin,
    suppress_context: bool,
    group_message: Option<String>,
}

/// Stable handle for an exception object owned by [`PythonExceptions`].
pub type PythonExceptionRef = ManagedHandle;

type ExceptionNode = ManagedException<PythonExceptionData, PythonExceptionRelation>;

/// Failure to construct or relate Python exception objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PythonExceptionError {
    /// The class was not declared in the Python class system.
    UnknownClass(ClassId),
    /// An exception handle is stale.
    Arena(ArenaError),
    /// A managed relation exceeded its checked limits.
    Relation(StrongEdgeMutationError),
    /// Python forbids empty exception groups.
    EmptyGroup,
    /// The referenced object is not an exception group.
    NotGroup,
}

impl From<ArenaError> for PythonExceptionError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}
impl From<StrongEdgeMutationError> for PythonExceptionError {
    fn from(value: StrongEdgeMutationError) -> Self {
        Self::Relation(value)
    }
}

#[derive(Debug)]
struct PythonExceptionFace {
    message: String,
}
impl Object for PythonExceptionFace {
    fn display(&self, _cx: &mut Cx) -> KernelResult<String> {
        Ok(self.message.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for PythonExceptionFace {}

/// Python exception heap, class policy, chaining, grouping, and handler matching.
pub struct PythonExceptions {
    classes: PythonObjectSpace,
    arena: ManagedArena<ExceptionNode>,
}

impl PythonExceptions {
    /// Construct a bounded Python exception heap.
    pub fn new(max_objects: usize) -> Result<Self, PythonExceptionError> {
        Ok(Self {
            classes: PythonObjectSpace::default(),
            arena: ManagedArena::new(HardCappedRetainPolicy::new(max_objects)?),
        })
    }

    /// Declare an exception class through the Python class system delivered by CLASS_2.
    pub fn define_class(
        &mut self,
        cx: &Cx,
        class: ClassRef,
        bases: Vec<ClassRef>,
    ) -> Result<(), crate::ClassError> {
        self.classes.define_class(cx, class, bases)
    }

    /// Allocate an ordinary exception object with exact traceback origin.
    pub fn allocate(
        &mut self,
        class: ClassRef,
        message: impl Into<String>,
        origin: Origin,
    ) -> Result<PythonExceptionRef, PythonExceptionError> {
        let id = class
            .object()
            .as_class()
            .map(|class| class.id())
            .ok_or(PythonExceptionError::UnknownClass(ClassId(u32::MAX)))?;
        if self.classes.class(id).is_none() {
            return Err(PythonExceptionError::UnknownClass(id));
        }
        Ok(self
            .arena
            .allocate(ManagedException::new(PythonExceptionData {
                class,
                message: message.into(),
                origin,
                suppress_context: false,
                group_message: None,
            }))?)
    }

    /// Allocate a non-empty exception group and retain members in source order.
    pub fn group(
        &mut self,
        class: ClassRef,
        message: impl Into<String>,
        members: &[PythonExceptionRef],
        origin: Origin,
    ) -> Result<PythonExceptionRef, PythonExceptionError> {
        if members.is_empty() {
            return Err(PythonExceptionError::EmptyGroup);
        }
        for member in members {
            self.arena.get(*member)?;
        }
        let group_message = message.into();
        let group = self.allocate(class, group_message.clone(), origin)?;
        let mut payload = self.arena.get(group)?.payload().clone();
        payload.group_message = Some(group_message);
        self.arena.get_mut(group)?.replace_payload(payload);
        for (ordinal, member) in members.iter().enumerate() {
            self.arena
                .get_mut(group)?
                .insert_relation(PythonExceptionRelation::GroupMember(ordinal), member.id())?;
        }
        Ok(group)
    }

    /// Attach an explicit cause and apply Python's context-suppression rule.
    pub fn set_cause(
        &mut self,
        error: PythonExceptionRef,
        cause: PythonExceptionRef,
    ) -> Result<(), PythonExceptionError> {
        self.arena.get(cause)?;
        let node = self.arena.get_mut(error)?;
        node.insert_relation(PythonExceptionRelation::Cause, cause.id())?;
        let mut payload = node.payload().clone();
        payload.suppress_context = true;
        node.replace_payload(payload);
        Ok(())
    }

    /// Attach the exception active when another exception was raised.
    pub fn set_context(
        &mut self,
        error: PythonExceptionRef,
        context: PythonExceptionRef,
    ) -> Result<(), PythonExceptionError> {
        self.arena.get(context)?;
        self.arena
            .get_mut(error)?
            .insert_relation(PythonExceptionRelation::Context, context.id())?;
        Ok(())
    }

    /// Convert a managed Python exception to the shared exceptional-completion envelope.
    pub fn raise(
        &self,
        cx: &Cx,
        error: PythonExceptionRef,
    ) -> Result<Raised, PythonExceptionError> {
        let payload = self.arena.get(error)?.payload();
        let value = cx
            .factory()
            .opaque(Arc::new(PythonExceptionFace {
                message: payload.message.clone(),
            }))
            .map_err(|_| PythonExceptionError::Arena(ArenaError::IdentityExhausted))?;
        Raised::new(
            payload.class.clone(),
            value,
            payload.origin.clone(),
            Symbol::qualified("python", "exception"),
        )
        .map_err(|_| PythonExceptionError::Arena(ArenaError::IdentityExhausted))
    }

    /// Match a raised completion using bounded class evidence and Python predicate policy.
    pub fn matches(
        &self,
        cx: &mut Cx,
        raised: &Raised,
        candidate: ClassRef,
        budget: ClassMatchBudget,
    ) -> ClassMatchOutcome {
        match_raised_class(
            cx,
            raised,
            candidate,
            budget,
            |_, actual, expected, budget| {
                let actual_id = actual
                    .object()
                    .as_class()
                    .expect("validated by matcher")
                    .id();
                let expected_id = expected
                    .object()
                    .as_class()
                    .expect("validated by matcher")
                    .id();
                let evidence = ClassMatchEvidence {
                    raised: actual_id,
                    candidate: expected_id,
                    performed_work: self
                        .classes
                        .subclass_work(actual_id, expected_id, budget.work),
                };
                if evidence.performed_work > budget.work {
                    BoundedSubclassOutcome::BudgetExhausted {
                        limit: budget.work,
                        performed_work: budget.work,
                    }
                } else if self.classes.is_subclass(actual_id, expected_id) {
                    BoundedSubclassOutcome::Subclass(evidence)
                } else {
                    BoundedSubclassOutcome::NotSubclass(evidence)
                }
            },
            |_, raised, _| Ok(raised.profile() == &Symbol::qualified("python", "exception")),
        )
    }

    /// Split a group by handler class while preserving direct-member order.
    pub fn split(
        &mut self,
        cx: &mut Cx,
        group: PythonExceptionRef,
        candidate: ClassRef,
        budget: ClassMatchBudget,
    ) -> Result<(Option<PythonExceptionRef>, Option<PythonExceptionRef>), PythonExceptionError>
    {
        let data = self.arena.get(group)?.payload().clone();
        let Some(message) = data.group_message.clone() else {
            return Err(PythonExceptionError::NotGroup);
        };
        let mut members = self
            .arena
            .get(group)?
            .relations()
            .filter_map(|(_, role, id)| match role {
                PythonExceptionRelation::GroupMember(ordinal) => {
                    Some((*ordinal, self.arena.handle(id).ok()?))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|(ordinal, _)| *ordinal);
        let mut matched = Vec::new();
        let mut rest = Vec::new();
        for (_, member) in members {
            let raised = self.raise(cx, member)?;
            if matches!(
                self.matches(cx, &raised, candidate.clone(), budget),
                ClassMatchOutcome::Matched(_)
            ) {
                matched.push(member);
            } else {
                rest.push(member);
            }
        }
        let make = |this: &mut Self,
                    values: &[PythonExceptionRef]|
         -> Result<Option<PythonExceptionRef>, PythonExceptionError> {
            if values.is_empty() {
                Ok(None)
            } else {
                this.group(
                    data.class.clone(),
                    message.clone(),
                    values,
                    data.origin.clone(),
                )
                .map(Some)
            }
        };
        let matched_group = make(self, &matched)?;
        let rest_group = make(self, &rest)?;
        Ok((matched_group, rest_group))
    }

    /// Return the immutable Python payload for diagnostics and policy checks.
    pub fn inspect(
        &self,
        error: PythonExceptionRef,
    ) -> Result<&PythonExceptionData, PythonExceptionError> {
        Ok(self.arena.get(error)?.payload())
    }

    /// Return ordered typed relations for diagnostics and subgroup derivation.
    pub fn relations(
        &self,
        error: PythonExceptionRef,
    ) -> Result<Vec<(PythonExceptionRelation, PythonExceptionRef)>, PythonExceptionError> {
        Ok(self
            .arena
            .get(error)?
            .relations()
            .map(|(_, role, id)| {
                (
                    *role,
                    self.arena
                        .handle(id)
                        .expect("managed relation targets a live object"),
                )
            })
            .collect())
    }
}

impl PythonExceptionData {
    /// Runtime exception class identity.
    pub fn class(&self) -> &ClassRef {
        &self.class
    }
    /// Exact guest diagnostic text.
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Traceback origin captured at construction.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }
    /// Whether implicit context display is suppressed.
    pub const fn suppress_context(&self) -> bool {
        self.suppress_context
    }
    /// Group message, present only for exception groups.
    pub fn group_message(&self) -> Option<&str> {
        self.group_message.as_deref()
    }
}

/// Policy seam for Python's synchronous context-manager protocol.
pub trait ContextManager<T> {
    /// Enter and produce the body value.
    fn enter(&mut self) -> Result<T, Box<Raised>>;
    /// Exit after normal or exceptional completion; `true` suppresses an exception.
    fn exit(&mut self, error: Option<&Raised>) -> Result<bool, Box<Raised>>;
}

/// Run one synchronous context extent, guaranteeing `exit` on both paths.
pub fn run_with_context<T, R>(
    manager: &mut impl ContextManager<T>,
    body: impl FnOnce(T) -> Result<R, Box<Raised>>,
) -> Result<Option<R>, Box<Raised>> {
    let entered = manager.enter()?;
    match body(entered) {
        Ok(value) => {
            manager.exit(None)?;
            Ok(Some(value))
        }
        Err(error) => {
            if manager.exit(Some(&error))? {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

/// Observable Python generator transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PythonGeneratorStep<T> {
    /// A value was yielded.
    Yielded(T),
    /// The generator returned; this is `StopIteration.value`.
    Returned(T),
}

/// Generator protocol or guest failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PythonGeneratorError {
    /// Shared frame protocol rejected the transition.
    Frame(FrameError),
    /// Guest exception escaped the frame.
    Raised(Box<Raised>),
}

/// Python send/throw/close policy backed by the shared resumable frame.
pub struct PythonGenerator<T, D> {
    frame: ResumableFrame<D>,
    _value: std::marker::PhantomData<T>,
}
impl<T, D> PythonGenerator<T, D>
where
    D: FnMut(
        ResumePacket<T, Raised>,
        &mut sim_lib_control::StepBudget,
    ) -> Result<ResumeResult<T, T, Raised>, FrameError>,
{
    /// Construct a bounded generator. This supplies no scheduler or event loop.
    pub fn new(limits: FrameLimits, driver: D) -> Self {
        Self {
            frame: ResumableFrame::new(limits, driver),
            _value: std::marker::PhantomData,
        }
    }
    /// Start execution and advance to the first yield.
    pub fn start(&mut self) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Start)
    }
    /// Send a value into the suspended generator.
    pub fn send(&mut self, value: T) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Send(value))
    }
    /// Throw an exception into the suspended generator.
    pub fn throw(&mut self, error: Raised) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Throw(error))
    }
    /// Close the suspended generator and run its driver cleanup.
    pub fn close(&mut self) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Close)
    }
    fn resume(
        &mut self,
        packet: ResumePacket<T, Raised>,
    ) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        match self
            .frame
            .resume(packet)
            .map_err(PythonGeneratorError::Frame)?
        {
            ResumeResult::Yielded(value) => Ok(PythonGeneratorStep::Yielded(value)),
            ResumeResult::Returned(value) => Ok(PythonGeneratorStep::Returned(value)),
            ResumeResult::Failed(error) => Err(PythonGeneratorError::Raised(Box::new(error))),
        }
    }
}

impl fmt::Display for PythonExceptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::{CodecId, DefaultFactory, NoopEvalPolicy, SourceId, Span};

    fn cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }
    fn class(cx: &Cx, id: u32, name: &str) -> ClassRef {
        cx.factory()
            .class_stub(ClassId(id), Symbol::qualified("python", name))
            .unwrap()
    }
    fn origin(at: usize) -> Origin {
        Origin {
            codec: CodecId(1),
            source: SourceId("exceptions3-python".into()),
            span: Span {
                start: at,
                end: at + 1,
            },
            trivia: Default::default(),
        }
    }

    #[test]
    fn managed_chains_groups_matching_and_diagnostics_preserve_python_policy() {
        let mut cx = cx();
        let mut exceptions = PythonExceptions::new(32).unwrap();
        let base = class(&cx, 1, "Exception");
        let key = class(&cx, 2, "KeyError");
        let runtime = class(&cx, 3, "RuntimeError");
        let group_class = class(&cx, 4, "ExceptionGroup");
        exceptions.define_class(&cx, base.clone(), vec![]).unwrap();
        for derived in [&key, &runtime, &group_class] {
            exceptions
                .define_class(&cx, derived.clone(), vec![base.clone()])
                .unwrap();
        }
        let cause = exceptions
            .allocate(runtime.clone(), "disk", origin(1))
            .unwrap();
        let explicit = exceptions.allocate(runtime, "outer", origin(2)).unwrap();
        exceptions.set_context(explicit, cause).unwrap();
        exceptions.set_cause(explicit, cause).unwrap();
        assert!(exceptions.inspect(explicit).unwrap().suppress_context());
        assert_eq!(exceptions.inspect(explicit).unwrap().origin().span.start, 2);
        let raised_key = exceptions
            .allocate(key.clone(), "missing", origin(3))
            .unwrap();
        let raised = exceptions.raise(&cx, raised_key).unwrap();
        assert!(matches!(
            exceptions.matches(&mut cx, &raised, base, ClassMatchBudget { work: 8 }),
            ClassMatchOutcome::Matched(_)
        ));
        assert!(matches!(
            exceptions.matches(&mut cx, &raised, key.clone(), ClassMatchBudget { work: 8 }),
            ClassMatchOutcome::Matched(_)
        ));
        assert_eq!(
            raised.payload().object().display(&mut cx).unwrap(),
            "missing"
        );
        assert_eq!(
            exceptions.group(group_class.clone(), "empty", &[], origin(4)),
            Err(PythonExceptionError::EmptyGroup)
        );
        let group = exceptions
            .group(group_class, "batch", &[explicit, raised_key], origin(5))
            .unwrap();
        let (matched, rest) = exceptions
            .split(&mut cx, group, key, ClassMatchBudget { work: 8 })
            .unwrap();
        let matched = matched.unwrap();
        let rest = rest.unwrap();
        assert_eq!(
            exceptions.relations(matched).unwrap(),
            vec![(PythonExceptionRelation::GroupMember(0), raised_key)]
        );
        assert_eq!(
            exceptions.relations(rest).unwrap(),
            vec![(PythonExceptionRelation::GroupMember(0), explicit)]
        );
        assert_eq!(
            exceptions.inspect(matched).unwrap().group_message(),
            Some("batch")
        );
    }

    #[test]
    fn generator_throws_only_shared_raised_envelopes() {
        let cx = cx();
        let mut exceptions = PythonExceptions::new(4).unwrap();
        let base = class(&cx, 1, "Exception");
        exceptions.define_class(&cx, base.clone(), vec![]).unwrap();
        let handle = exceptions.allocate(base, "boom", origin(7)).unwrap();
        let raised = exceptions.raise(&cx, handle).unwrap();
        let mut generator = PythonGenerator::new(FrameLimits { depth: 1, work: 2 }, |packet, _| {
            Ok(match packet {
                ResumePacket::Start => ResumeResult::Yielded(0),
                ResumePacket::Throw(error) => ResumeResult::Failed(error),
                ResumePacket::Send(value) => ResumeResult::Yielded(value),
                ResumePacket::Close => ResumeResult::Returned(0),
            })
        });
        generator.start().unwrap();
        assert!(matches!(
            generator.throw(raised),
            Err(PythonGeneratorError::Raised(_))
        ));
    }
}
