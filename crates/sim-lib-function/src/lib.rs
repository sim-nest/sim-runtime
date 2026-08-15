#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Shared function-plan contract.
//!
//! The implementation is intentionally introduced by later FUNCTION_2 phases.
//! Its frozen ownership and migration boundary is recorded in `CONTRACT.md`.
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
