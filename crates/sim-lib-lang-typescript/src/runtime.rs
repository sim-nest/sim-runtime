use sim_kernel::{Expr, Result};
use sim_lib_lang_javascript::{Completion, JavascriptEvalPolicy, JavascriptState};

use crate::AnnotationMetadata;

/// Codec-produced program: one erased JavaScript graph plus observational metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeScriptProgram {
    /// Direct JavaScript lowering supplied by `codec/typescript`.
    pub javascript: Expr,
    /// Annotation provenance and admitted Shape projections.
    pub annotations: Vec<AnnotationMetadata>,
}

/// Notation adapter delegating every evaluation step to JavaScript.
#[derive(Clone, Debug)]
pub struct TypeScriptNotation {
    javascript: JavascriptEvalPolicy,
}

impl TypeScriptNotation {
    /// Construct the adapter with the JavaScript evaluator's ordinary step bound.
    pub fn new(max_steps: usize) -> Result<Self> {
        Ok(Self {
            javascript: JavascriptEvalPolicy::new(max_steps)?,
        })
    }

    /// Evaluate only the erased graph. Annotation records are intentionally unread.
    pub fn eval(
        &self,
        program: &TypeScriptProgram,
        state: &mut JavascriptState,
    ) -> Result<Completion> {
        self.javascript.eval_lowered(&program.javascript, state)
    }
}
