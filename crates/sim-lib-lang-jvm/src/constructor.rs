//! Alias-stable constructor state for the static-checked execution tier.

use std::{collections::BTreeMap, error::Error, fmt};

use sim_lib_machine::SourceLocation;
use sim_lib_mutation::{ManagedHandle, ManagedId};

use crate::{InvocationKind, JvmReference, VerificationFidelity};

/// The operation attempting to consume an allocated-but-uninitialized reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UninitializedUse {
    /// A field, array, monitor, cast, return, or ordinary invocation use.
    Ordinary,
    /// Method completion from an instance constructor.
    ConstructorReturn,
    /// A constructor call made with an invocation mode other than `invokespecial`.
    ConstructorInvocation,
    /// An `invokespecial` call whose selected member is not `<init>`.
    NonConstructorSpecial,
}

/// Located constructor-state refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstructorStateError {
    allocation: Box<SourceLocation>,
    use_location: Box<SourceLocation>,
    attempted: UninitializedUse,
}

impl ConstructorStateError {
    /// Exact `new` instruction (or constructor-entry receiver) that created the state.
    pub const fn allocation_location(&self) -> &SourceLocation {
        &self.allocation
    }

    /// Exact instruction at which the invalid use was detected.
    pub const fn use_location(&self) -> &SourceLocation {
        &self.use_location
    }

    /// Kind of use refused.
    pub const fn attempted_use(&self) -> UninitializedUse {
        self.attempted
    }
}

impl fmt::Display for ConstructorStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "uninitialized reference allocated at {:?} was refused at {:?} ({:?}); static-checked fidelity can trap after earlier admitted effects",
            self.allocation, self.use_location, self.attempted
        )
    }
}

impl Error for ConstructorStateError {}

#[derive(Clone, Debug)]
struct AllocationState {
    location: SourceLocation,
    initialized: bool,
}

/// Per-execution constructor state keyed by managed identity.
///
/// Locals and operand-stack aliases carry the same [`ManagedId`], so copying a
/// reference cannot lose its state. This is deliberately a dynamic/static-admission
/// guard, not a control-flow verifier: a refusal can occur after earlier effects.
#[derive(Clone, Debug, Default)]
pub struct ConstructorState {
    allocations: BTreeMap<ManagedId, AllocationState>,
    constructor_receiver: Option<ManagedId>,
}

impl ConstructorState {
    /// Records the result of `new` before the reference becomes guest-visible.
    pub fn allocated(&mut self, handle: ManagedHandle, location: SourceLocation) {
        self.allocations.insert(
            handle.id(),
            AllocationState {
                location,
                initialized: false,
            },
        );
    }

    /// Records the uninitialized receiver supplied on entry to `<init>`.
    pub fn constructor_entry(&mut self, handle: ManagedHandle, location: SourceLocation) {
        self.allocated(handle, location);
        self.constructor_receiver = Some(handle.id());
    }

    /// Refuses an ordinary use until the matching constructor has completed.
    pub fn require_initialized(
        &self,
        reference: JvmReference,
        use_location: SourceLocation,
    ) -> Result<(), ConstructorStateError> {
        self.require(reference, use_location, UninitializedUse::Ordinary)
    }

    /// Applies constructor invocation rules and initializes every alias on success.
    pub fn invoke(
        &mut self,
        reference: JvmReference,
        kind: InvocationKind,
        selected_name: &str,
        use_location: SourceLocation,
    ) -> Result<(), ConstructorStateError> {
        let Some(handle) = reference.handle() else {
            return Ok(());
        };
        let Some(state) = self.allocations.get_mut(&handle.id()) else {
            return Ok(());
        };
        if state.initialized {
            return Ok(());
        }
        let attempted = if selected_name != "<init>" {
            UninitializedUse::NonConstructorSpecial
        } else if kind != InvocationKind::Special {
            UninitializedUse::ConstructorInvocation
        } else {
            state.initialized = true;
            return Ok(());
        };
        Err(ConstructorStateError {
            allocation: Box::new(state.location.clone()),
            use_location: Box::new(use_location),
            attempted,
        })
    }

    /// Refuses return from `<init>` while its receiver remains uninitialized.
    pub fn constructor_return(
        &self,
        use_location: SourceLocation,
    ) -> Result<(), ConstructorStateError> {
        let Some(receiver) = self.constructor_receiver else {
            return Ok(());
        };
        let Some(state) = self.allocations.get(&receiver) else {
            return Ok(());
        };
        if state.initialized {
            Ok(())
        } else {
            Err(ConstructorStateError {
                allocation: Box::new(state.location.clone()),
                use_location: Box::new(use_location),
                attempted: UninitializedUse::ConstructorReturn,
            })
        }
    }

    /// Fidelity honestly established by this guard without a formal verifier.
    pub const fn fidelity(&self) -> VerificationFidelity {
        VerificationFidelity::StaticChecked
    }

    /// Attaches a later verifier proof to this same state path.
    pub const fn strengthen<P>(&self, proof: P) -> VerifiedConstructorState<'_, P> {
        VerifiedConstructorState { state: self, proof }
    }

    fn require(
        &self,
        reference: JvmReference,
        use_location: SourceLocation,
        attempted: UninitializedUse,
    ) -> Result<(), ConstructorStateError> {
        let Some(handle) = reference.handle() else {
            return Ok(());
        };
        match self.allocations.get(&handle.id()) {
            Some(state) if !state.initialized => Err(ConstructorStateError {
                allocation: Box::new(state.location.clone()),
                use_location: Box::new(use_location),
                attempted,
            }),
            _ => Ok(()),
        }
    }
}

/// A verifier proof attached to the exact constructor-state path used at execution.
pub struct VerifiedConstructorState<'a, P> {
    state: &'a ConstructorState,
    proof: P,
}

impl<P> VerifiedConstructorState<'_, P> {
    /// Constructor state strengthened by this proof.
    pub const fn state(&self) -> &ConstructorState {
        self.state
    }

    /// Provider-owned proof.
    pub const fn proof(&self) -> &P {
        &self.proof
    }

    /// Fidelity after the verifier has strengthened the shared path.
    pub const fn fidelity(&self) -> VerificationFidelity {
        VerificationFidelity::Verified
    }
}
