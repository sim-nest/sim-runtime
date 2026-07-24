#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Generic incremental query calculation for SIM runtime libraries.
//!
//! `sim-incremental-core` records dependencies from actual query execution. A
//! query can read another query through its [`QueryFrame`], observe external
//! stamps, and return any hashable Rust value. The engine keeps memoized values,
//! reverse dependency edges, cycle paths, typed budget failures, continuation
//! tokens, and bounded graph snapshots without depending on SIM expressions,
//! codecs, Table/Dir storage, web surfaces, or expression-tree records.
//!
//! # Examples
//!
//! ```
//! use sim_incremental_core::{IncrementalEngine, QueryResult};
//!
//! let mut engine = IncrementalEngine::<&'static str, i64>::new();
//! engine.register_fn("a", |_, _| Ok(1));
//! engine.register_fn("b", |_, frame| {
//!     let a = frame.read("a")?;
//!     Ok(a + 1)
//! });
//!
//! let value: QueryResult<_, _> = engine.verify("b");
//! assert_eq!(value.unwrap(), 2);
//! ```

mod budget;
mod engine;
mod error;
mod fingerprint;
mod observation;
mod query;
mod snapshot;
mod state;

pub use budget::{BudgetKind, QueryBudgets, SnapshotBudgets};
pub use engine::{IncrementalEngine, QueryFrame};
pub use error::{ContinuationToken, IncrementalError, SnapshotError};
pub use fingerprint::{FingerprintValue, ValueFingerprint};
pub use observation::{Observation, ObservationKind, Revision};
pub use query::{Query, QueryResult};
pub use snapshot::{GraphSnapshot, RestoreReport, SnapshotNode};

#[cfg(test)]
mod tests;
