use sim_kernel::{Args, Cx, Result, Value};

/// Result of invoking a callable through a protected boundary.
///
/// Protected calls turn ordinary kernel call failures into language-neutral
/// raised values supplied by the caller's error mapper.
#[derive(Clone, Debug)]
pub enum ProtectedOutcome {
    /// The callable returned normally.
    Returned(Vec<Value>),
    /// The callable raised a mapped error value.
    Raised(Value),
}

/// Calls `function` and maps kernel errors into a returned protected outcome.
///
/// The kernel callable surface returns one value per call. The protected result
/// stores successful values in a vector so language layers with multi-value
/// returns can reuse the same outcome type at their boundary.
pub fn protected_call(
    cx: &mut Cx,
    function: Value,
    args: Args,
    map_error: impl FnOnce(&mut Cx, sim_kernel::Error) -> Result<Value>,
) -> Result<ProtectedOutcome> {
    match cx.call_value(function, args) {
        Ok(value) => Ok(ProtectedOutcome::Returned(vec![value])),
        Err(error) => Ok(ProtectedOutcome::Raised(map_error(cx, error)?)),
    }
}
