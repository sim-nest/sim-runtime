use sim_kernel::{
    Cx, Error, Ref, Result, Symbol,
    control::{ControlCapture, capture, default_control_result_shape},
};

use crate::ContinuationValue;

/// A signalled condition: a kind symbol plus a payload reference.
///
/// The condition system's analogue of a raised exception, but resumable: the
/// payload travels to the nearest matching [`ConditionHandler`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    kind: Symbol,
    payload: Ref,
}

impl Condition {
    /// Builds a condition of the given `kind` carrying `payload`.
    pub fn new(kind: Symbol, payload: Ref) -> Self {
        Self { kind, payload }
    }

    /// Returns the condition kind used to select a handler.
    pub fn kind(&self) -> &Symbol {
        &self.kind
    }

    /// Returns the payload delivered to the handler.
    pub fn payload(&self) -> &Ref {
        &self.payload
    }
}

/// A handler bound to a condition kind, prompt, and capture continuation.
///
/// Installed on a [`ConditionStack`]; when a matching [`Condition`] is
/// signalled, its prompt and continuation drive the underlying control capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionHandler {
    kind: Symbol,
    prompt: Ref,
    continuation: Ref,
    multishot: bool,
}

impl ConditionHandler {
    /// Builds a one-shot handler for `kind` bound to `prompt` and
    /// `continuation`.
    pub fn new(kind: Symbol, prompt: Ref, continuation: Ref) -> Self {
        Self {
            kind,
            prompt,
            continuation,
            multishot: false,
        }
    }

    /// Returns a copy of this handler marked multishot (resumable more than
    /// once).
    pub fn multishot(mut self) -> Self {
        self.multishot = true;
        self
    }

    /// Returns the condition kind this handler matches.
    pub fn kind(&self) -> &Symbol {
        &self.kind
    }

    /// Returns the prompt the handler captures against.
    pub fn prompt(&self) -> &Ref {
        &self.prompt
    }

    /// Returns the continuation resumed after the handler runs.
    pub fn continuation(&self) -> &Ref {
        &self.continuation
    }
}

/// A stack of [`ConditionHandler`]s, searched innermost-first on signal.
///
/// Models the dynamic-extent handler chain of the condition system; signalling
/// dispatches to the nearest handler whose kind matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConditionStack {
    handlers: Vec<ConditionHandler>,
}

impl ConditionStack {
    /// Builds an empty condition stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs `handler` as the new innermost handler.
    pub fn push(&mut self, handler: ConditionHandler) {
        self.handlers.push(handler);
    }

    /// Removes and returns the innermost handler, if any.
    pub fn pop(&mut self) -> Option<ConditionHandler> {
        self.handlers.pop()
    }

    /// Signals `condition`, capturing against the nearest matching handler and
    /// returning the resulting [`ContinuationValue`].
    ///
    /// Fails with [`Error::Eval`](sim_kernel::Error::Eval) when no handler
    /// matches the condition kind.
    pub fn signal(&self, cx: &mut Cx, condition: Condition) -> Result<ContinuationValue> {
        let handler = self.nearest_handler(condition.kind())?;
        let mut request = ControlCapture::new(
            handler.prompt().clone(),
            handler.continuation().clone(),
            condition.payload().clone(),
            default_control_result_shape(),
        );
        if handler.multishot {
            request = request.multishot();
        }
        let capture_result = capture(cx, request)?;
        Ok(ContinuationValue::new(
            handler.continuation().clone(),
            capture_result,
            handler.multishot,
        ))
    }

    /// Returns the innermost handler matching `kind`, or
    /// [`Error::Eval`](sim_kernel::Error::Eval) when none is installed.
    pub fn nearest_handler(&self, kind: &Symbol) -> Result<&ConditionHandler> {
        self.handlers
            .iter()
            .rev()
            .find(|handler| handler.kind() == kind)
            .ok_or_else(|| Error::Eval(format!("no condition handler for {kind}")))
    }
}

/// Signals `condition` against `stack`; free-function form of
/// [`ConditionStack::signal`].
pub fn signal_condition(
    cx: &mut Cx,
    stack: &ConditionStack,
    condition: Condition,
) -> Result<ContinuationValue> {
    stack.signal(cx, condition)
}
