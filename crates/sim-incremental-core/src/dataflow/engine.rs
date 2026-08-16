//! Stable, budgeted worklist propagation.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use crate::{BudgetKind, ContinuationToken, FingerprintValue, QueryBudgets, ValueFingerprint};

use super::{AdmittedTransfer, DataflowGraph, EdgeClass, JoinSemilattice, TransferPolicy};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("engine/contracts.rs");
include!("engine/execution.rs");
include!("engine/proof.rs");
include!("engine/tests.rs");
