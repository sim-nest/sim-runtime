use sim_lib_journal::JournalError;
use thiserror::Error;

/// Typed refusal from durable operation construction, replay, or publication.
#[derive(Debug, Error)]
pub enum OperationError {
    /// An already entered accepted operation cannot dispatch or reconcile again.
    #[error("accepted operation was already entered")]
    AcceptanceConsumed,
    /// A sticky cancellation forbids new execution, not independent observation.
    #[error("operation cancellation prohibits further execution")]
    Cancelled,
    /// Resource reservation is unavailable; the durable dispatch still needs reconciliation.
    #[error("operation preparation unavailable: {0:?}")]
    PreparationUnavailable(sim_kernel::Datum),
    /// Preparation acknowledgement failed and its original owner's disposition
    /// also refused. Neither error may be erased or converted into completion.
    #[error("{cause}; preparation failure disposition: {disposition}")]
    PreparationDisposition {
        /// Original acknowledgement failure, retained without reclassification.
        #[source]
        cause: Box<OperationError>,
        /// Original owner's refusal to acknowledge failure disposition.
        disposition: Box<OperationError>,
    },
    /// An operation name cannot be empty.
    #[error("operation name is empty")]
    EmptyOperation,
    /// A supplied value was not the canonical value claimed by its identity.
    #[error("noncanonical {0}")]
    NonCanonical(&'static str),
    /// The same claimed operation identity carried different intent.
    #[error("contradictory intent under one operation id")]
    ContradictoryIntent,
    /// A grant did not belong to the stable operation.
    #[error("operation grant does not match intent")]
    GrantMismatch,
    /// An attempt did not belong to the stable operation.
    #[error("operation attempt does not match intent")]
    AttemptMismatch,
    /// A bounded operation lease was empty, expired at acquisition, or moved backwards.
    #[error("invalid bounded operation lease")]
    InvalidLease,
    /// The postcondition observer was the same authority as the performer.
    #[error("postcondition observer is not independent of the performer")]
    ObserverNotIndependent,
    /// Stored success was checked by a different observer contract.
    #[error("operation success requires fresh observation under the selected observer contract")]
    ObserverMismatch,
    /// Recovery selected a different performer authority for the same operation.
    #[error("operation performer does not match the durable dispatch authority")]
    PerformerMismatch,
    /// Durable replay contained a second intent for one operation.
    #[error("durable operation contains duplicate intent")]
    DuplicateIntent,
    /// A durable event violated the operation state order.
    #[error("invalid durable operation transition: {0}")]
    InvalidTransition(&'static str),
    /// The service has no live fenced writer lease.
    #[error("operation service must be explicitly resumed")]
    NotResumed,
    /// The journal sequence cannot be extended.
    #[error("operation journal sequence is exhausted")]
    SequenceExhausted,
    /// The underlying canonical journal refused the operation.
    #[error(transparent)]
    Journal(#[from] JournalError),
}
