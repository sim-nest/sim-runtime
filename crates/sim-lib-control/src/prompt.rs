use sim_kernel::{
    Cx, Ref, Result, Symbol,
    control::{ControlPrompt as KernelControlPrompt, default_control_result_shape, prompt},
};

/// Stable tag identifying a delimited control prompt.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlTag {
    symbol: Symbol,
}

impl ControlTag {
    /// Builds a control tag from a symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }

    /// Returns the symbol carried by this tag.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Consumes this tag and returns its symbol.
    pub fn into_symbol(self) -> Symbol {
        self.symbol
    }
}

/// Library-level contract for prompts raised by runtime organs.
pub trait ControlPrompt {
    /// Returns the stable tag identifying the prompt kind.
    fn tag(&self) -> ControlTag;

    /// Returns the input reference supplied to the prompt body.
    fn input(&self) -> Ref {
        Ref::Symbol(self.tag().into_symbol())
    }

    /// Returns the shape reference expected for the prompt result.
    fn result_shape(&self) -> Ref {
        default_control_result_shape()
    }
}

/// Raises a library-level control prompt through the kernel control contract.
pub fn raise_prompt(cx: &mut Cx, prompt_record: &dyn ControlPrompt) -> Result<Ref> {
    let kernel_prompt = KernelControlPrompt::new(
        Ref::Symbol(prompt_record.tag().into_symbol()),
        prompt_record.input(),
        prompt_record.result_shape(),
    );
    let input = kernel_prompt.input.clone();
    prompt(cx, kernel_prompt, |_cx| Ok(input))
}
