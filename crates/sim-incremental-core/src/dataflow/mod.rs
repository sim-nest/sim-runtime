//! Deterministic, located dataflow graphs.

mod engine;
mod graph;
mod lattice;

pub use engine::{
    DataflowError, DataflowEvent, DataflowFailure, DataflowResult, DataflowSolution, DataflowUsage,
    FixpointEngine,
};
pub use graph::{
    AdaptedGraph, AdapterBuildResult, Boundary, DataflowGraph, Edge, EdgeClass, EdgeSpec,
    GraphBuildError, GraphDirection, LocatedGraphAdapter, Node, NodeSpec,
};
pub use lattice::{
    AdmittedTransfer, DataflowLaw, JoinSemilattice, LawSuite, LawViolation, StateSize,
    TransferPolicy,
};
