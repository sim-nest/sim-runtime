use sim_kernel::{
    Cx, Ref, Result,
    control::{ControlAbort, ControlPrompt, abort, default_control_result_shape, prompt},
};

use crate::clojure_recur_prompt_symbol;

/// Returns the control-prompt [`Ref`] that delimits a Clojure `loop`/`recur` target.
pub fn clojure_loop_prompt_ref() -> Ref {
    Ref::Symbol(clojure_recur_prompt_symbol())
}

/// Runs `body` inside the Clojure `loop` prompt so a nested [`clojure_recur`] can rebind it.
///
/// Maps the surface `loop` form onto a kernel control prompt; behavior lives in
/// the control organ, not this profile.
pub fn clojure_loop_prompt<F>(cx: &mut Cx, input: Ref, body: F) -> Result<Ref>
where
    F: FnOnce(&mut Cx) -> Result<Ref>,
{
    prompt(
        cx,
        ControlPrompt::new(
            clojure_loop_prompt_ref(),
            input,
            default_control_result_shape(),
        ),
        body,
    )
}

/// Aborts to the enclosing Clojure `loop` prompt with new bindings, implementing `recur`.
pub fn clojure_recur(cx: &mut Cx, value: Ref) -> Result<Ref> {
    abort(
        cx,
        ControlAbort::new(
            clojure_loop_prompt_ref(),
            value,
            default_control_result_shape(),
        ),
    )
}
