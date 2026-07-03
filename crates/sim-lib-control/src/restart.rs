use sim_kernel::{
    Cx, Error, Ref, Result, Symbol,
    control::{ControlResume, default_control_result_shape, resume},
};

use crate::{ContinuationValue, ControlResultValue};

/// A named recovery point: a symbol bound to a captured continuation.
///
/// The condition system's restart: invoking it resumes the underlying
/// [`ContinuationValue`] with a supplied value, returning control to the point
/// where the restart was established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restart {
    name: Symbol,
    continuation: ContinuationValue,
}

impl Restart {
    /// Binds `name` to the recovery `continuation`.
    pub fn new(name: Symbol, continuation: ContinuationValue) -> Self {
        Self { name, continuation }
    }

    /// Returns the restart name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Returns the continuation this restart resumes.
    pub fn continuation(&self) -> &ContinuationValue {
        &self.continuation
    }

    /// Invokes the restart, resuming its continuation with `value` and
    /// returning the resulting [`ControlResultValue`].
    pub fn invoke(&self, cx: &mut Cx, value: Ref) -> Result<ControlResultValue> {
        let result = resume(
            cx,
            ControlResume::new(
                self.continuation.continuation().clone(),
                value,
                default_control_result_shape(),
            ),
        )?;
        Ok(ControlResultValue::new(result))
    }
}

/// A stack of [`Restart`]s, searched innermost-first by name on invocation.
///
/// Models the dynamic-extent restart chain of the condition system.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestartStack {
    restarts: Vec<Restart>,
}

impl RestartStack {
    /// Builds an empty restart stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs `restart` as the new innermost restart.
    pub fn push(&mut self, restart: Restart) {
        self.restarts.push(restart);
    }

    /// Removes and returns the innermost restart, if any.
    pub fn pop(&mut self) -> Option<Restart> {
        self.restarts.pop()
    }

    /// Invokes the nearest restart named `name` with `value`.
    ///
    /// Fails with [`Error::Eval`](sim_kernel::Error::Eval) when no restart with
    /// that name is installed.
    pub fn invoke(&self, cx: &mut Cx, name: &Symbol, value: Ref) -> Result<ControlResultValue> {
        self.nearest_restart(name)?.invoke(cx, value)
    }

    /// Returns the innermost restart named `name`, or
    /// [`Error::Eval`](sim_kernel::Error::Eval) when none matches.
    pub fn nearest_restart(&self, name: &Symbol) -> Result<&Restart> {
        self.restarts
            .iter()
            .rev()
            .find(|restart| restart.name() == name)
            .ok_or_else(|| Error::Eval(format!("no restart named {name}")))
    }
}

/// Invokes the restart named `name` on `stack` with `value`; free-function
/// form of [`RestartStack::invoke`].
pub fn invoke_restart(
    cx: &mut Cx,
    stack: &RestartStack,
    name: &Symbol,
    value: Ref,
) -> Result<ControlResultValue> {
    stack.invoke(cx, name, value)
}
