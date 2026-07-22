use sim_kernel::{Args, Cx, Result, Value};

/// A value paired with the callable that closes it at scope exit.
#[derive(Clone, Debug)]
pub struct CloseGuard {
    /// The value being closed.
    pub value: Value,
    /// The callable invoked to close `value`.
    pub close_fn: Value,
}

impl CloseGuard {
    /// Builds a guard for `value` using `close_fn` as the close operation.
    pub fn new(value: Value, close_fn: Value) -> Self {
        Self { value, close_fn }
    }
}

/// Runs `body` and closes all guards in reverse order before returning.
///
/// Each close function receives the guarded value and a pending-error argument.
/// The pending-error argument is `nil` on a normal return, or a string
/// representation of the body error when `body` fails. All guards are invoked
/// even when an earlier guard reports an error. Body errors take precedence
/// over close errors; otherwise the first close error is returned.
pub fn run_with_close_guards(
    cx: &mut Cx,
    guards: Vec<CloseGuard>,
    body: impl FnOnce(&mut Cx) -> Result<Value>,
) -> Result<Value> {
    let result = body(cx);
    let pending_error = pending_error_value(cx, &result)?;
    let mut first_close_error = None;

    for guard in guards.into_iter().rev() {
        let close_result = cx.call_value(
            guard.close_fn,
            Args::new(vec![guard.value, pending_error.clone()]),
        );
        if let Err(error) = close_result {
            first_close_error.get_or_insert(error);
        }
    }

    match result {
        Ok(value) => match first_close_error {
            Some(error) => Err(error),
            None => Ok(value),
        },
        Err(error) => Err(error),
    }
}

fn pending_error_value(cx: &mut Cx, result: &Result<Value>) -> Result<Value> {
    match result {
        Ok(_) => cx.factory().nil(),
        Err(error) => cx.factory().string(error.to_string()),
    }
}
