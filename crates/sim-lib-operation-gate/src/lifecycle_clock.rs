//! Explicit clock-domain bindings for durable lease admission.

use crate::OperationError;
use sim_kernel::Datum;

/// A reading from a boot-selected clock owner, not caller-minted authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseClockReading {
    /// Canonical domain including epoch and tick-unit identity.
    pub domain: Datum,
    /// Monotonic tick in that domain.
    pub tick: u64,
}

/// Independent current-time authority selected by the operation's native owner.
///
/// Domain identity includes the epoch and units. Reopening a clock cannot reset
/// its origin while keeping the same identity. A read is bounded and performs
/// no release, lease extension, resource acquisition or journal mutation.
pub trait LeaseClock: Send + Sync {
    /// Returns a freshly verified domain and tick, or refuses unavailable time.
    fn read(&self) -> Result<LeaseClockReading, OperationError>;
}
