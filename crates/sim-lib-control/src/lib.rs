#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Control behavior for the SIM runtime: async, backtracking, conditions.
//!
//! The kernel defines the control-policy contracts; this crate supplies the
//! concrete control organ (coroutines, generators, restarts, non-local exits)
//! layered over those contracts.

mod r#async;
mod backtrack;
mod claims;
mod condition;
mod conditional;
mod coroutine;
mod generator;
mod model;
mod nonlocal;
mod ops;
mod policy;
mod prompt;
mod restart;
mod runtime;

pub use r#async::{AsyncPoll, AsyncTask};
pub use backtrack::{BacktrackStep, Backtracker};
pub use claims::{
    control_op_keys, control_organ_symbol, publish_control_organ_claims,
    publish_control_organ_claims_for_lib,
};
pub use condition::{Condition, ConditionHandler, ConditionStack, signal_condition};
pub use conditional::IfForm;
pub use coroutine::{Coroutine, CoroutineLane, CoroutineStep};
pub use generator::{Generator, GeneratorStep};
pub use model::{ContinuationValue, ControlResultValue};
pub use nonlocal::{LabeledPrompt, NonLocalExit, NonLocalExitKind, escape_to_label};
pub use ops::{ControlFunction, abort_symbol, capture_symbol, prompt_symbol, resume_symbol};
pub use policy::{
    OneShotControlPolicy, SegmentedControlPolicy, install_control_policy, one_shot_control_policy,
    segmented_control_policy,
};
pub use prompt::{ControlPrompt, ControlTag, raise_prompt};
pub use restart::{Restart, RestartStack, invoke_restart};
pub use runtime::{ControlLib, control_exports, install_control_lib, manifest_name};

#[cfg(test)]
mod derivation_tests;

#[cfg(test)]
mod tests;
