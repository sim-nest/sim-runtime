#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Policy contracts for neutral, bounded decoded-instruction machines.
//!
//! This crate intentionally provides no policy implementation and no execution
//! engine. Consumers supply every semantic choice through the traits below.

mod slots;
mod stack;

pub use slots::{SlotError, SlotFile};
pub use stack::{StackError, UnitStack};

/// Supplies stable instruction identity and the consumer's decoded form.
///
/// For example, a WebAssembly decoder or a BEAM loader can supply this policy.
pub trait InstructionPolicy {
    /// One decoded instruction.
    type Instruction;
    /// Stable identity used by branches, coverage, and receipts.
    type InstructionId: Copy + Eq + Ord;

    /// Returns the stable identity of `instruction`.
    fn instruction_id(instruction: &Self::Instruction) -> Self::InstructionId;
}

/// Accounts values in consumer-defined logical storage units.
///
/// For example, a WebAssembly value policy or a Forth cell policy can supply it.
pub trait ValueWidthPolicy {
    /// A machine value whose representation remains consumer-owned.
    type Value;

    /// Returns the nonzero logical width charged for `value`.
    fn width(value: &Self::Value) -> usize;
}

/// Classifies the effects an instruction may request from its driver.
///
/// For example, a WebAssembly embedder or an Erlang emulator can supply it.
pub trait EffectPolicy<I> {
    /// Consumer-defined effect description.
    type Effect;

    /// Describes the effect of `instruction` without performing it.
    fn classify(instruction: &I) -> Self::Effect;
}

/// Describes bounded guest-frame metadata without owning frame storage.
///
/// For example, a WebAssembly engine or a PostScript interpreter can supply it.
pub trait FramePolicy {
    /// Consumer-defined frame metadata.
    type Frame;
    /// Stable callable identity.
    type CallableId: Copy + Eq + Ord;

    /// Returns the callable associated with `frame`.
    fn callable(frame: &Self::Frame) -> Self::CallableId;
}

/// Selects protected regions and consumer-defined abrupt outcomes.
///
/// For example, a WebAssembly trap policy or a BEAM exit policy can supply it.
pub trait HandlerPolicy<I> {
    /// Consumer-defined handler identity.
    type HandlerId: Copy + Eq + Ord;
    /// Consumer-defined abrupt outcome.
    type Abrupt;

    /// Finds the handler, if any, for an instruction and abrupt outcome.
    fn handler_for(instruction: I, abrupt: &Self::Abrupt) -> Option<Self::HandlerId>;
}

/// Projects live machine state into the managed-root owner's identity type.
///
/// For example, a WebAssembly reference policy or a Scheme heap can supply it.
pub trait RootPolicy<S> {
    /// Root identity understood by the consumer's managed arena.
    type Root;

    /// Visits every live root in deterministic order.
    fn visit_roots(state: &S, visit: impl FnMut(&Self::Root));
}

/// Declares semantic polling locations in prepared code.
///
/// For example, a WebAssembly loop policy or a Lua interpreter can supply it.
pub trait SafepointPolicy<I> {
    /// Returns whether the instruction is a semantic safepoint.
    fn is_safepoint(instruction: &I) -> bool;
}

/// Performs effect-free validation and issues a content-bound permit.
///
/// For example, a WebAssembly validator or an eBPF verifier can supply it.
pub trait AdmissionPolicy<C> {
    /// Permit proving that the supplied code and limits were admitted.
    type Permit;
    /// Structured refusal evidence.
    type Refusal;

    /// Validates `code` without executing consumer effects.
    fn admit(code: &C) -> Result<Self::Permit, Self::Refusal>;
}

/// Creates deterministic evidence for bounded machine work.
///
/// For example, a WebAssembly audit profile or a Forth tracer can supply it.
pub trait ReceiptPolicy<E> {
    /// Consumer-defined receipt.
    type Receipt;

    /// Records exact work from ordered execution evidence.
    fn receipt(evidence: E) -> Self::Receipt;
}
