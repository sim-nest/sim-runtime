use sim_lib_journal::JournalError;
use thiserror::Error;

/// Typed refusal from durable operation construction, replay, or publication.
#[derive(Debug, Error)]
pub enum OperationError {
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
