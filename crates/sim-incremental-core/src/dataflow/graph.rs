//! Immutable graph construction and neutral located-code adaptation.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use crate::{FingerprintValue, ValueFingerprint};

/// The semantic class of an edge.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EdgeClass<C> {
    /// Ordinary value or fact propagation.
    Data,
    /// Ordering or control-flow propagation.
    Control,
    /// A consumer-defined edge class.
    Custom(C),
}

/// The direction in which an edge propagates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GraphDirection {
    /// Propagate from the edge's source to its target.
    Forward,
    /// Propagate from the edge's target to its source.
    Reverse,
}

/// A node's declared relationship to the graph boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Boundary {
    /// The node is internal to the graph.
    Internal,
    /// The node may receive facts from outside the graph.
    Input,
    /// The node may send facts outside the graph.
    Output,
    /// The node is both an input and an output.
    InputOutput,
}

impl Boundary {
    fn is_input(self) -> bool {
        matches!(self, Self::Input | Self::InputOutput)
    }

    fn is_output(self) -> bool {
        matches!(self, Self::Output | Self::InputOutput)
    }
}

/// A node supplied to checked graph construction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeSpec<N, L> {
    /// Stable node identity.
    pub id: N,
    /// Consumer-neutral source or artifact location.
    pub location: L,
    /// Declared graph-boundary role.
    pub boundary: Boundary,
}

/// An edge supplied to checked graph construction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EdgeSpec<E, N, C> {
    /// Stable edge identity.
    pub id: E,
    /// Declared source node.
    pub source: N,
    /// Declared target node.
    pub target: N,
    /// Semantic edge class.
    pub class: EdgeClass<C>,
    /// Propagation direction.
    pub direction: GraphDirection,
}

/// One immutable graph node.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Node<N, L> {
    id: N,
    location: L,
    boundary: Boundary,
}

impl<N, L> Node<N, L> {
    /// Returns the stable node identity.
    pub fn id(&self) -> &N {
        &self.id
    }

    /// Returns the consumer-neutral location.
    pub fn location(&self) -> &L {
        &self.location
    }

    /// Returns the declared boundary role.
    pub fn boundary(&self) -> Boundary {
        self.boundary
    }
}

/// One immutable graph edge.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Edge<E, N, C> {
    id: E,
    source: N,
    target: N,
    class: EdgeClass<C>,
    direction: GraphDirection,
}

impl<E, N, C> Edge<E, N, C> {
    /// Returns the stable edge identity.
    pub fn id(&self) -> &E {
        &self.id
    }

    /// Returns the declared source node.
    pub fn source(&self) -> &N {
        &self.source
    }

    /// Returns the declared target node.
    pub fn target(&self) -> &N {
        &self.target
    }

    /// Returns the semantic edge class.
    pub fn class(&self) -> &EdgeClass<C> {
        &self.class
    }

    /// Returns the propagation direction.
    pub fn direction(&self) -> GraphDirection {
        self.direction
    }

    fn predecessor_and_successor(&self) -> (&N, &N) {
        match self.direction {
            GraphDirection::Forward => (&self.source, &self.target),
            GraphDirection::Reverse => (&self.target, &self.source),
        }
    }
}

/// A checked graph-construction refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphBuildError<N, E> {
    /// A stable node identity was supplied more than once.
    DuplicateNode(N),
    /// A stable edge identity was supplied more than once.
    DuplicateEdge(E),
    /// An edge names a node absent from the graph.
    MissingNode {
        /// Rejected edge identity.
        edge: E,
        /// Missing endpoint identity.
        node: N,
    },
    /// An input boundary has an in-graph predecessor.
    InputHasPredecessor(N),
    /// An output boundary has an in-graph successor.
    OutputHasSuccessor(N),
    /// A graph without nodes cannot carry a meaningful identity.
    Empty,
}

/// Immutable, canonically ordered, content-identified dataflow structure.
///
/// The generic parameters represent graph identities, locations, and edge
/// classes only. Adapters may project machine code, syntax trees, or other
/// located artifacts into these neutral values without storing their source
/// types in the graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataflowGraph<N, E, L, C> {
    nodes: BTreeMap<N, Node<N, L>>,
    edges: BTreeMap<E, Edge<E, N, C>>,
    predecessors: BTreeMap<N, Box<[E]>>,
    successors: BTreeMap<N, Box<[E]>>,
    fingerprint: ValueFingerprint,
}

impl<N, E, L, C> DataflowGraph<N, E, L, C>
where
    N: Clone + Hash + Ord,
    E: Clone + Hash + Ord,
    L: Hash + Ord,
    C: Hash + Ord,
{
    /// Validates and freezes graph declarations in canonical identity order.
    pub fn build(
        nodes: impl IntoIterator<Item = NodeSpec<N, L>>,
        edges: impl IntoIterator<Item = EdgeSpec<E, N, C>>,
    ) -> Result<Self, GraphBuildError<N, E>> {
        let mut frozen_nodes = BTreeMap::new();
        for node in nodes {
            let id = node.id.clone();
            let node = Node {
                id: node.id,
                location: node.location,
                boundary: node.boundary,
            };
            if frozen_nodes.insert(id.clone(), node).is_some() {
                return Err(GraphBuildError::DuplicateNode(id));
            }
        }
        if frozen_nodes.is_empty() {
            return Err(GraphBuildError::Empty);
        }

        let mut frozen_edges = BTreeMap::new();
        for edge in edges {
            let id = edge.id.clone();
            for endpoint in [&edge.source, &edge.target] {
                if !frozen_nodes.contains_key(endpoint) {
                    return Err(GraphBuildError::MissingNode {
                        edge: id,
                        node: endpoint.clone(),
                    });
                }
            }
            let edge = Edge {
                id: edge.id,
                source: edge.source,
                target: edge.target,
                class: edge.class,
                direction: edge.direction,
            };
            if frozen_edges.insert(id.clone(), edge).is_some() {
                return Err(GraphBuildError::DuplicateEdge(id));
            }
        }

        let mut predecessors = frozen_nodes
            .keys()
            .cloned()
            .map(|id| (id, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut successors = predecessors.clone();
        for (edge_id, edge) in &frozen_edges {
            let (predecessor, successor) = edge.predecessor_and_successor();
            successors
                .get_mut(predecessor)
                .expect("validated edge source exists")
                .insert(edge_id.clone());
            predecessors
                .get_mut(successor)
                .expect("validated edge target exists")
                .insert(edge_id.clone());
        }
        for (id, node) in &frozen_nodes {
            if node.boundary.is_input() && !predecessors[id].is_empty() {
                return Err(GraphBuildError::InputHasPredecessor(id.clone()));
            }
            if node.boundary.is_output() && !successors[id].is_empty() {
                return Err(GraphBuildError::OutputHasSuccessor(id.clone()));
            }
        }

        let fingerprint = (&frozen_nodes, &frozen_edges).incremental_fingerprint();
        Ok(Self {
            nodes: frozen_nodes,
            edges: frozen_edges,
            predecessors: freeze_index(predecessors),
            successors: freeze_index(successors),
            fingerprint,
        })
    }

    /// Returns a node by stable identity.
    pub fn node(&self, id: &N) -> Option<&Node<N, L>> {
        self.nodes.get(id)
    }

    /// Returns an edge by stable identity.
    pub fn edge(&self, id: &E) -> Option<&Edge<E, N, C>> {
        self.edges.get(id)
    }

    /// Iterates nodes in stable identity order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &Node<N, L>> {
        self.nodes.values()
    }

    /// Iterates edges in stable identity order.
    pub fn edges(&self) -> impl ExactSizeIterator<Item = &Edge<E, N, C>> {
        self.edges.values()
    }

    /// Returns incoming edge identities in stable order.
    pub fn predecessors(&self, node: &N) -> Option<&[E]> {
        self.predecessors.get(node).map(Box::as_ref)
    }

    /// Returns outgoing edge identities in stable order.
    pub fn successors(&self, node: &N) -> Option<&[E]> {
        self.successors.get(node).map(Box::as_ref)
    }

    /// Returns the canonical content fingerprint.
    pub fn fingerprint(&self) -> ValueFingerprint {
        self.fingerprint
    }
}

fn freeze_index<K: Ord, V: Ord>(index: BTreeMap<K, BTreeSet<V>>) -> BTreeMap<K, Box<[V]>> {
    index
        .into_iter()
        .map(|(key, values)| (key, values.into_iter().collect()))
        .collect()
}

/// Projects an external located representation into neutral graph declarations.
///
/// Implement this trait on a small adapter that borrows an external located-code
/// object. Only the returned identities, locations, classes, and directions are
/// retained by [`DataflowGraph`].
pub trait LocatedGraphAdapter {
    /// Stable node identity.
    type NodeId: Clone + Hash + Ord;
    /// Stable edge identity.
    type EdgeId: Clone + Hash + Ord;
    /// Neutral location value.
    type Location: Hash + Ord;
    /// Consumer edge-class extension.
    type Class: Hash + Ord;

    /// Produces located node declarations.
    fn nodes(&self) -> Vec<NodeSpec<Self::NodeId, Self::Location>>;

    /// Produces directed edge declarations.
    fn edges(&self) -> Vec<EdgeSpec<Self::EdgeId, Self::NodeId, Self::Class>>;

    /// Validates and freezes the projected graph.
    fn build_graph(&self) -> AdapterBuildResult<Self> {
        DataflowGraph::build(self.nodes(), self.edges())
    }
}

/// The neutral graph produced by a particular located adapter.
pub type AdaptedGraph<A> = DataflowGraph<
    <A as LocatedGraphAdapter>::NodeId,
    <A as LocatedGraphAdapter>::EdgeId,
    <A as LocatedGraphAdapter>::Location,
    <A as LocatedGraphAdapter>::Class,
>;

/// The checked construction result produced by a located adapter.
pub type AdapterBuildResult<A> = Result<
    AdaptedGraph<A>,
    GraphBuildError<<A as LocatedGraphAdapter>::NodeId, <A as LocatedGraphAdapter>::EdgeId>,
>;

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8, boundary: Boundary) -> NodeSpec<u8, (u8, u8)> {
        NodeSpec {
            id,
            location: (id, id + 1),
            boundary,
        }
    }

    fn edge(id: u8, source: u8, target: u8) -> EdgeSpec<u8, u8, &'static str> {
        EdgeSpec {
            id,
            source,
            target,
            class: EdgeClass::Data,
            direction: GraphDirection::Forward,
        }
    }

    #[test]
    fn insertion_order_does_not_change_structure_or_fingerprint() {
        let left = DataflowGraph::build(
            [
                node(1, Boundary::Input),
                node(2, Boundary::Internal),
                node(3, Boundary::Output),
            ],
            [edge(10, 1, 2), edge(20, 2, 3)],
        )
        .unwrap();
        let right = DataflowGraph::build(
            [
                node(3, Boundary::Output),
                node(1, Boundary::Input),
                node(2, Boundary::Internal),
            ],
            [edge(20, 2, 3), edge(10, 1, 2)],
        )
        .unwrap();

        assert_eq!(left, right);
        assert_eq!(left.fingerprint(), right.fingerprint());
        assert_eq!(left.successors(&1), Some([10].as_slice()));
        assert_eq!(left.predecessors(&3), Some([20].as_slice()));
    }

    #[test]
    fn rejects_duplicate_missing_and_invalid_boundary_declarations() {
        assert_eq!(
            DataflowGraph::<_, u8, _, &str>::build(
                [node(1, Boundary::Internal), node(1, Boundary::Internal)],
                [],
            ),
            Err(GraphBuildError::DuplicateNode(1))
        );
        assert!(matches!(
            DataflowGraph::build([node(1, Boundary::Internal)], [edge(7, 1, 2)]),
            Err(GraphBuildError::MissingNode { edge: 7, node: 2 })
        ));
        assert_eq!(
            DataflowGraph::build(
                [node(1, Boundary::Internal), node(2, Boundary::Input)],
                [edge(7, 1, 2)],
            ),
            Err(GraphBuildError::InputHasPredecessor(2))
        );
        assert_eq!(
            DataflowGraph::build(
                [node(1, Boundary::Output), node(2, Boundary::Internal)],
                [edge(7, 1, 2)],
            ),
            Err(GraphBuildError::OutputHasSuccessor(1))
        );
    }

    #[test]
    fn graph_public_surface_remains_representation_neutral() {
        let source = include_str!("graph.rs");
        let public_surface = source
            .lines()
            .filter(|line| line.trim_start().starts_with("pub "))
            .collect::<String>();
        for forbidden in ["Machine", "Jvm", "JVM", "LocatedCode"] {
            assert!(
                !public_surface.contains(forbidden),
                "public graph surface names {forbidden}"
            );
        }
    }
}
