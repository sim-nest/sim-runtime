#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Language-neutral immutable function plans.
//!
//! A [`FunctionPlan`] records declaration metadata only. It deliberately has
//! no evaluation or invocation surface; later instance machinery composes a
//! plan with a concrete guest policy.
//!
//! A body remains a statically selected guest policy. Type erasure cannot meet
//! the bound of the future instance type:
//!
//! ```compile_fail
//! use std::any::Any;
//!
//! trait GuestBodyPolicy {}
//! struct FunctionInstance<B: GuestBodyPolicy>(B);
//!
//! fn erase(body: Box<dyn Any>) -> FunctionInstance<Box<dyn Any>> {
//!     FunctionInstance(body)
//! }
//! ```

mod bind;
mod instance;
mod plan;

pub use bind::{ArgumentInput, ArgumentOrigin, BoundArgument, BoundCall, CallInput, bind};
pub use instance::{CapturedBinding, FunctionBodyPolicy, FunctionInstance, InstanceError};
pub use plan::{
    BrowseProjection, CallMode, CaptureDescriptor, FunctionPlan, ParameterDescriptor,
    ParameterKind, PlanError,
};
