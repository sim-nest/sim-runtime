//! Python exception and resumable-control policy over the shared control organ.

use sim_lib_control::{FrameError, FrameLimits, ResumableFrame, ResumePacket, ResumeResult};

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

/// A checked Python exception with explicit chaining fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonException {
    /// Exception class name.
    pub class: String,
    /// Exception message.
    pub message: String,
    /// Explicit `raise ... from ...` cause.
    pub cause: Option<Box<PythonException>>,
    /// Implicit active-exception context.
    pub context: Option<Box<PythonException>>,
    /// Whether implicit context display is suppressed.
    pub suppress_context: bool,
}

impl PythonException {
    /// Construct an unchained exception.
    pub fn new(class: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            message: message.into(),
            cause: None,
            context: None,
            suppress_context: false,
        }
    }
    /// Attach an explicit cause, suppressing implicit context display.
    pub fn with_cause(mut self, cause: PythonException) -> Self {
        self.cause = Some(Box::new(cause));
        self.suppress_context = true;
        self
    }
    /// Attach the exception active while this exception was raised.
    pub fn with_context(mut self, context: PythonException) -> Self {
        self.context = Some(Box::new(context));
        self
    }
}

/// A non-empty nested Python exception group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonExceptionGroup {
    /// Group message.
    pub message: String,
    /// Direct exceptions in stable order.
    pub exceptions: Vec<PythonException>,
}
impl PythonExceptionGroup {
    /// Construct a group, rejecting the empty case.
    pub fn new(
        message: impl Into<String>,
        exceptions: Vec<PythonException>,
    ) -> Result<Self, PythonException> {
        if exceptions.is_empty() {
            Err(PythonException::new(
                "ValueError",
                "exception group must be non-empty",
            ))
        } else {
            Ok(Self {
                message: message.into(),
                exceptions,
            })
        }
    }
    /// Split matching exception classes while preserving order.
    pub fn split(self, class: &str) -> (Option<Self>, Option<Self>) {
        let (matched, rest): (Vec<_>, Vec<_>) = self
            .exceptions
            .into_iter()
            .partition(|error| error.class == class);
        let make = |exceptions: Vec<_>| {
            (!exceptions.is_empty()).then(|| Self {
                message: self.message.clone(),
                exceptions,
            })
        };
        (make(matched), make(rest))
    }
}

/// Policy seam for Python's synchronous context-manager protocol.
pub trait ContextManager<T> {
    /// Enter and produce the body value.
    fn enter(&mut self) -> Result<T, PythonException>;
    /// Exit after normal or exceptional completion; `true` suppresses an exception.
    fn exit(&mut self, error: Option<&PythonException>) -> Result<bool, PythonException>;
}

/// Run one synchronous context extent, guaranteeing `exit` on both paths.
pub fn run_with_context<T, R>(
    manager: &mut impl ContextManager<T>,
    body: impl FnOnce(T) -> Result<R, PythonException>,
) -> Result<Option<R>, PythonException> {
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
    Raised(PythonException),
}

/// Python send/throw/close policy backed by the shared resumable frame.
pub struct PythonGenerator<T, D> {
    frame: ResumableFrame<D>,
    _value: std::marker::PhantomData<T>,
}
impl<T, D> PythonGenerator<T, D>
where
    D: FnMut(
        ResumePacket<T, PythonException>,
        &mut sim_lib_control::StepBudget,
    ) -> Result<ResumeResult<T, T, PythonException>, FrameError>,
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
    pub fn throw(
        &mut self,
        error: PythonException,
    ) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Throw(error))
    }
    /// Close the suspended generator and run its driver cleanup.
    pub fn close(&mut self) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        self.resume(ResumePacket::Close)
    }
    fn resume(
        &mut self,
        packet: ResumePacket<T, PythonException>,
    ) -> Result<PythonGeneratorStep<T>, PythonGeneratorError> {
        match self
            .frame
            .resume(packet)
            .map_err(PythonGeneratorError::Frame)?
        {
            ResumeResult::Yielded(value) => Ok(PythonGeneratorStep::Yielded(value)),
            ResumeResult::Returned(value) => Ok(PythonGeneratorStep::Returned(value)),
            ResumeResult::Failed(error) => Err(PythonGeneratorError::Raised(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[test]
    fn generator_composes_start_send_throw_and_close() {
        let mut iterator = PythonIterator::new(vec![1, 2]);
        assert_eq!(iterator.next_checked(), Some(1));
        assert_eq!(iterator.next_checked(), Some(2));
        assert_eq!(iterator.next_checked(), None);
        let cleaned = Rc::new(RefCell::new(false));
        let mark = cleaned.clone();
        let mut generator =
            PythonGenerator::new(FrameLimits { depth: 4, work: 8 }, move |packet, budget| {
                budget.charge_work()?;
                Ok(match packet {
                    ResumePacket::Start => ResumeResult::Yielded(1),
                    ResumePacket::Send(value) => ResumeResult::Yielded(value + 1),
                    ResumePacket::Throw(error) => ResumeResult::Failed(error),
                    ResumePacket::Close => {
                        *mark.borrow_mut() = true;
                        ResumeResult::Returned(0)
                    }
                })
            });
        assert_eq!(generator.start(), Ok(PythonGeneratorStep::Yielded(1)));
        assert_eq!(generator.send(4), Ok(PythonGeneratorStep::Yielded(5)));
        assert_eq!(generator.close(), Ok(PythonGeneratorStep::Returned(0)));
        assert!(*cleaned.borrow());

        let mut throwing = PythonGenerator::new(FrameLimits { depth: 1, work: 1 }, |packet, _| {
            Ok(match packet {
                ResumePacket::Start => ResumeResult::Yielded(0),
                ResumePacket::Throw(error) => ResumeResult::Failed(error),
                ResumePacket::Send(value) => ResumeResult::Yielded(value),
                ResumePacket::Close => ResumeResult::Returned(0),
            })
        });
        throwing.start().unwrap();
        assert!(matches!(
            throwing.throw(PythonException::new("KeyError", "x")),
            Err(PythonGeneratorError::Raised(_))
        ));
    }

    #[test]
    fn chaining_groups_and_context_cleanup_are_checked() {
        let root = PythonException::new("OSError", "root");
        let chained = PythonException::new("RuntimeError", "outer")
            .with_context(root.clone())
            .with_cause(root);
        assert!(chained.suppress_context);
        let group = PythonExceptionGroup::new(
            "many",
            vec![chained, PythonException::new("TypeError", "bad")],
        )
        .unwrap();
        assert_eq!(group.split("TypeError").0.unwrap().exceptions.len(), 1);
        struct Manager(bool);
        impl ContextManager<i32> for Manager {
            fn enter(&mut self) -> Result<i32, PythonException> {
                Ok(42)
            }
            fn exit(&mut self, _: Option<&PythonException>) -> Result<bool, PythonException> {
                self.0 = true;
                Ok(false)
            }
        }
        let mut manager = Manager(false);
        assert_eq!(run_with_context(&mut manager, Ok), Ok(Some(42)));
        assert!(manager.0);
    }
}
