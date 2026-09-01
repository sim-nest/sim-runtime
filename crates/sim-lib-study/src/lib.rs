//! Durable, domain-neutral study coordination.
//!
//! Selection is pure. Authority is a sealed content object plus append-only
//! journal facts. Executor effects occur outside journal transactions and only
//! a reply bound to the live claim can become terminal evidence.

#![forbid(unsafe_code)]

pub mod decision;
pub mod design;
pub mod product;

use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::{Journal, JournalBackend, JournalEntry, JournalError, JournalObject, Lease};
use sim_study_core::{AttemptOutcome, StudyCoordinate, StudyError, SubjectRevision};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use thiserror::Error;

mod lifecycle;

pub use lifecycle::*;

#[cfg(test)]
mod tests;
