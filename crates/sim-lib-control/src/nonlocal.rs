use sim_kernel::{
    Cx, Error, Ref, Result, Symbol,
    control::{ControlAbort, abort, default_control_result_shape},
};

use crate::ControlResultValue;

/// The flavor of a non-local exit: loop break, loop continue, or block return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonLocalExitKind {
    /// Exit the enclosing labeled loop.
    Break,
    /// Continue to the next iteration of the enclosing labeled loop.
    Next,
    /// Return from the enclosing labeled block.
    Return,
}

/// A labeled escape target: a label symbol bound to a control prompt.
///
/// Names a dynamic-extent landing site so a [`NonLocalExit`] can abort to the
/// matching prompt by label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabeledPrompt {
    label: Symbol,
    prompt: Ref,
}

impl LabeledPrompt {
    /// Binds `label` to the escape `prompt`.
    pub fn new(label: Symbol, prompt: Ref) -> Self {
        Self { label, prompt }
    }

    /// Returns the escape label.
    pub fn label(&self) -> &Symbol {
        &self.label
    }

    /// Returns the prompt this label aborts to.
    pub fn prompt(&self) -> &Ref {
        &self.prompt
    }
}

/// A pending non-local exit: a kind, a target label, and a carried value.
///
/// Resolved by [`escape_to_label`], which aborts to the [`LabeledPrompt`] whose
/// label matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NonLocalExit {
    kind: NonLocalExitKind,
    label: Symbol,
    value: Ref,
}

impl NonLocalExit {
    /// Builds a non-local exit of `kind` targeting `label` and carrying
    /// `value`.
    pub fn new(kind: NonLocalExitKind, label: Symbol, value: Ref) -> Self {
        Self { kind, label, value }
    }

    /// Builds a [`NonLocalExitKind::Break`] exit to `label` with `value`.
    pub fn break_to(label: Symbol, value: Ref) -> Self {
        Self::new(NonLocalExitKind::Break, label, value)
    }

    /// Builds a [`NonLocalExitKind::Next`] exit to `label` with `value`.
    pub fn next_to(label: Symbol, value: Ref) -> Self {
        Self::new(NonLocalExitKind::Next, label, value)
    }

    /// Builds a [`NonLocalExitKind::Return`] exit to `label` with `value`.
    pub fn return_to(label: Symbol, value: Ref) -> Self {
        Self::new(NonLocalExitKind::Return, label, value)
    }

    /// Returns the exit kind.
    pub fn kind(&self) -> NonLocalExitKind {
        self.kind
    }

    /// Returns the target label.
    pub fn label(&self) -> &Symbol {
        &self.label
    }

    /// Returns the value carried out of the block.
    pub fn value(&self) -> &Ref {
        &self.value
    }
}

/// Performs `exit` by aborting to the matching [`LabeledPrompt`] in `prompts`.
///
/// Searches `prompts` innermost-first for the exit's label and aborts to it,
/// returning the [`ControlResultValue`] the abort produces. Fails with
/// [`Error::Eval`](sim_kernel::Error::Eval) when no labeled prompt matches.
pub fn escape_to_label(
    cx: &mut Cx,
    prompts: &[LabeledPrompt],
    exit: NonLocalExit,
) -> Result<ControlResultValue> {
    let prompt = prompts
        .iter()
        .rev()
        .find(|prompt| prompt.label() == exit.label())
        .ok_or_else(|| Error::Eval(format!("no labeled prompt for {}", exit.label())))?;
    let result = abort(
        cx,
        ControlAbort::new(
            prompt.prompt().clone(),
            exit.value().clone(),
            default_control_result_shape(),
        ),
    )?;
    Ok(ControlResultValue::new(result))
}
