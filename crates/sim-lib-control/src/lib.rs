#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Control behavior for the SIM runtime: async, backtracking, conditions.
//!
//! The kernel defines the control-policy contracts; this crate supplies the
//! concrete control organ (coroutines, generators, restarts, non-local exits)
//! layered over those contracts.
//!
//! ## Raising guest values
//!
//! Every guest raises the non-recursive [`Raised`] envelope. The guest obtains
//! the envelope's class identity from its declared class descriptor, retains
//! the ordinary guest value as the payload, and stores cause, context, group,
//! or suppression relationships as stable edges in [`ManagedException`].
//! Handlers select that class through [`match_raised_class`]; neither a guest
//! language nor a host adapter needs another throwable record or relation
//! chain.

mod r#async;
mod backtrack;
mod claims;
mod close;
mod condition;
mod conditional;
mod coroutine;
mod exception;
mod generator;
mod jobs;
mod matching;
mod model;
mod nonlocal;
mod ops;
mod policy;
mod prompt;
mod protected;
mod restart;
mod resume;
mod runtime;
mod unwind;

#[cfg(test)]
mod exception_ownership_guard {
    include!("../tests/exceptions3_carrier_ownership.rs");
}

pub use r#async::{AsyncPoll, AsyncTask};
pub use backtrack::{BacktrackStep, Backtracker};
pub use claims::{
    control_op_keys, control_organ_symbol, publish_control_organ_claims,
    publish_control_organ_claims_for_lib,
};
pub use close::{CloseGuard, run_with_close_guards};
pub use condition::{Condition, ConditionHandler, ConditionStack, signal_condition};
pub use conditional::IfForm;
pub use coroutine::{Coroutine, CoroutineFrame, CoroutineFrameStep, CoroutineLane, CoroutineStep};
pub use exception::{
    ExceptionGraphBudget, ExceptionGraphEdge, ExceptionGraphView, ManagedException,
};
pub use generator::{Generator, GeneratorStep};
pub use jobs::{
    AdmissionLimit, CheckpointError, CheckpointReceipt, DrainReceipt, JobId, JobQueues, JobReceipt,
    JobStatus, RuntimeJobClass, WorkLimit,
};
pub use matching::BoundedSubclassOutcome;
pub use matching::{ClassMatchBudget, ClassMatchEvidence, ClassMatchOutcome, match_raised_class};
pub use model::{
    ContinuationValue, ControlResultValue, RAISED_SYMBOL, Raised, RaisedBrowseBudget,
    RaisedBrowseProjection, RaisedShape,
};
pub use nonlocal::{LabeledPrompt, NonLocalExit, NonLocalExitKind, escape_to_label};
pub use ops::{
    ControlFunction, abort_symbol, capture_symbol, physical_sensing_trace_symbol, prompt_symbol,
    resume_symbol,
};
pub use policy::{
    OneShotControlPolicy, SegmentedControlPolicy, install_control_policy, one_shot_control_policy,
    segmented_control_policy,
};
pub use prompt::{ControlPrompt, ControlTag, raise_prompt};
pub use protected::{ProtectedOutcome, protected_call, protected_call_with};
pub use restart::{Restart, RestartStack, invoke_restart};
pub use resume::{FrameError, FrameLimits, ResumableFrame, ResumePacket, ResumeResult, StepBudget};
pub use runtime::{ControlLib, control_exports, install_control_lib, manifest_name};
pub use unwind::{CleanupStack, Unwind};

/// Exceptional unwind specialized to the shared raised envelope.
pub type RaisedUnwind<R, B, C> = Unwind<R, B, C, Raised>;
/// Protected-call result specialized to the shared raised envelope.
pub type RaisedProtectedOutcome = ProtectedOutcome<Raised>;
/// Condition payload specialized to the shared raised envelope.
pub type RaisedCondition = Condition<Raised>;
/// Resume input specialized to throwing the shared raised envelope.
pub type RaisedResumePacket<T> = ResumePacket<T, Raised>;
/// Resume result specialized to failure with the shared raised envelope.
pub type RaisedResumeResult<T, R> = ResumeResult<T, R, Raised>;

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod derivation_tests;

#[cfg(test)]
mod frame_tests;

#[cfg(test)]
mod organ_tests;

#[cfg(test)]
mod tests;
