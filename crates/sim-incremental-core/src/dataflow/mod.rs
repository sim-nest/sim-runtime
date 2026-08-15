//! Deterministic, located dataflow graphs.

mod graph;
mod lattice;

pub use graph::{
    AdaptedGraph, AdapterBuildResult, Boundary, DataflowGraph, Edge, EdgeClass, EdgeSpec,
    GraphBuildError, GraphDirection, LocatedGraphAdapter, Node, NodeSpec,
};
pub use lattice::{
    AdmittedTransfer, DataflowLaw, JoinSemilattice, LawSuite, LawViolation, StateSize,
    TransferPolicy,
};
