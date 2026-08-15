//! Deterministic, located dataflow graphs.

mod graph;

pub use graph::{
    AdaptedGraph, AdapterBuildResult, Boundary, DataflowGraph, Edge, EdgeClass, EdgeSpec,
    GraphBuildError, GraphDirection, LocatedGraphAdapter, Node, NodeSpec,
};
