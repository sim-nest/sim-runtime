use sim_kernel::{Error, Result};

pub(crate) fn logic_eval_error(message: impl Into<String>) -> Error {
    Error::Eval(message.into())
}

pub(crate) fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(logic_eval_error(message))
    }
}
