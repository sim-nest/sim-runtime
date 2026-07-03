use sim_kernel::{Cx, Ref, Result, Symbol};
use sim_lib_control::{ControlResultValue, LabeledPrompt, NonLocalExit, escape_to_label};

/// A labeled Ruby block scope targeted by `break` and `next` exits.
///
/// Lowers Ruby block control onto the control organ's labeled-prompt contract:
/// the scope pairs a label with the control prompt that bounds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubyBlockScope {
    label: Symbol,
    prompt: Ref,
}

impl RubyBlockScope {
    /// Creates a block scope for `label`, deriving its control prompt.
    pub fn new(label: Symbol) -> Self {
        let prompt = Ref::Symbol(Symbol::qualified("ruby/block-prompt", label.to_string()));
        Self { label, prompt }
    }

    /// Returns the scope's label.
    pub fn label(&self) -> &Symbol {
        &self.label
    }

    /// Returns the control prompt that bounds this scope.
    pub fn prompt(&self) -> &Ref {
        &self.prompt
    }

    /// Returns the [`LabeledPrompt`] pairing this scope's label and prompt.
    pub fn labeled_prompt(&self) -> LabeledPrompt {
        LabeledPrompt::new(self.label.clone(), self.prompt.clone())
    }
}

/// Performs a Ruby `break`, escaping the block scope with `value`.
///
/// Delegates to the control organ's labeled non-local exit rather than defining
/// bespoke control behavior.
pub fn ruby_break(cx: &mut Cx, scope: &RubyBlockScope, value: Ref) -> Result<ControlResultValue> {
    escape_to_label(
        cx,
        &[scope.labeled_prompt()],
        NonLocalExit::break_to(scope.label.clone(), value),
    )
}

/// Performs a Ruby `next`, advancing the block scope with `value`.
///
/// Delegates to the control organ's labeled non-local exit rather than defining
/// bespoke control behavior.
pub fn ruby_next(cx: &mut Cx, scope: &RubyBlockScope, value: Ref) -> Result<ControlResultValue> {
    escape_to_label(
        cx,
        &[scope.labeled_prompt()],
        NonLocalExit::next_to(scope.label.clone(), value),
    )
}
