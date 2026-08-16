//! Incremental engine session object and SIM expression conversion.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use sim_incremental_core::{
    GraphSnapshot, IncrementalEngine, IncrementalError, Observation, ObservationKind, QueryFrame,
    QueryResult, SnapshotBudgets,
    dataflow::{
        DATAFLOW_PROOF_SCHEMA_REVISION, DataflowCompletionProof, DataflowEvent, DataflowGraph,
        EdgeClass,
    },
};
use sim_kernel::{
    Cx, Dir, Error, Expr, Object, ObjectCompat, Result, Symbol, Table, Value,
    id::CORE_TABLE_CLASS_ID, object::ClassRef,
};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("model/analysis_view.rs");
include!("model/session.rs");
