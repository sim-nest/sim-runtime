use std::collections::{BTreeMap, BTreeSet, VecDeque};

use sim_lib_mutation::{
    ArenaError, EdgeId, EdgeLimits, EdgeSnapshot, EdgeVisitor, ManagedArena, ManagedHandle,
    ManagedId, ManagedNode, ManagedObject, StrongEdgeMutationError,
};

/// A guest exception payload with caller-defined relation roles.
///
/// The payload is the open role carried by the shared [`ManagedNode`]. Relation
/// roles are deliberately caller data: this adapter does not prescribe cause,
/// context, group, or suppression semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedException<P, R> {
    node: ManagedNode<P>,
    relation_roles: BTreeMap<EdgeId, R>,
}

impl<P, R> ManagedException<P, R> {
    /// Creates an exception payload with the managed node's standard edge limits.
    pub const fn new(payload: P) -> Self {
        Self::with_edge_limits(payload, EdgeLimits::DEFAULT)
    }

    /// Creates an exception payload with explicit managed edge limits.
    pub const fn with_edge_limits(payload: P, limits: EdgeLimits) -> Self {
        Self {
            node: ManagedNode::with_edge_limits(payload, limits),
            relation_roles: BTreeMap::new(),
        }
    }

    /// Borrows the guest payload.
    pub const fn payload(&self) -> &P {
        self.node.role()
    }

    /// Replaces the guest payload without changing relation identity.
    pub fn replace_payload(&mut self, payload: P) -> P {
        self.node.replace_role(payload)
    }

    /// Adds a retaining relation with caller-owned role evidence.
    pub fn insert_relation(
        &mut self,
        role: R,
        target: ManagedId,
    ) -> Result<EdgeId, StrongEdgeMutationError> {
        let edge = self.node.insert_strong(target)?;
        let previous = self.relation_roles.insert(edge, role);
        debug_assert!(
            previous.is_none(),
            "fresh managed edge must not have a role"
        );
        Ok(edge)
    }

    /// Removes exactly the expected relation and returns its role and target.
    pub fn remove_relation(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
    ) -> Result<(R, ManagedId), StrongEdgeMutationError> {
        let target = self.node.remove_strong(edge, expected)?;
        let role = self
            .relation_roles
            .remove(&edge)
            .expect("every adapter relation has role evidence");
        Ok((role, target))
    }

    /// Returns relation edges in stable edge-id order.
    pub fn relations(&self) -> impl Iterator<Item = (EdgeId, &R, ManagedId)> {
        self.node
            .edge_snapshot()
            .into_iter()
            .filter_map(|snapshot| {
                let EdgeSnapshot::Strong { edge, target } = snapshot else {
                    return None;
                };
                Some((edge, &self.relation_roles[&edge], target))
            })
    }
}

impl<P, R> ManagedObject for ManagedException<P, R> {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        self.node.trace_edges(visitor);
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        self.node.clear_weak_edge(edge, expected)
    }

    fn clear_ephemeron_edge(
        &mut self,
        edge: EdgeId,
        expected_key: ManagedId,
        expected_value: ManagedId,
    ) -> bool {
        self.node
            .clear_ephemeron_edge(edge, expected_key, expected_value)
    }
}

/// Maximum number of relation edges admitted to one graph projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExceptionGraphBudget {
    max_edges: usize,
}

impl ExceptionGraphBudget {
    /// Creates an edge budget. Zero is useful for a presence-only probe.
    pub const fn new(max_edges: usize) -> Self {
        Self { max_edges }
    }

    /// Returns the admitted edge count.
    pub const fn max_edges(self) -> usize {
        self.max_edges
    }
}

/// One caller-typed relation in a bounded graph view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExceptionGraphEdge<R> {
    /// Parent object that owns the stable edge identity.
    pub parent: ManagedId,
    /// Stable identity local to `parent`.
    pub edge: EdgeId,
    /// Caller-owned relation role.
    pub role: R,
    /// Related exception object.
    pub target: ManagedId,
}

/// Terminating graph projection with explicit loss reporting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExceptionGraphView<R> {
    /// Relations in deterministic parent-discovery and edge-id order.
    pub edges: Vec<ExceptionGraphEdge<R>>,
    /// True when at least one relation was omitted by the edge budget.
    pub truncated: bool,
}

impl<R: Clone> ExceptionGraphView<R> {
    /// Projects reachable relations iteratively, expanding each object once.
    ///
    /// Every parent edge remains a row even when multiple parents target the
    /// same object. The expanded set only prevents cycles from recurring.
    pub fn project<P>(
        arena: &ManagedArena<ManagedException<P, R>>,
        root: ManagedHandle,
        budget: ExceptionGraphBudget,
    ) -> Result<Self, ArenaError> {
        arena.get(root)?;
        let mut queue = VecDeque::from([root.id()]);
        let mut expanded = BTreeSet::new();
        let mut edges = Vec::new();

        while let Some(parent) = queue.pop_front() {
            if !expanded.insert(parent) {
                continue;
            }
            let handle = arena.handle(parent)?;
            for (edge, role, target) in arena.get(handle)?.relations() {
                if edges.len() == budget.max_edges {
                    return Ok(Self {
                        edges,
                        truncated: true,
                    });
                }
                edges.push(ExceptionGraphEdge {
                    parent,
                    edge,
                    role: role.clone(),
                    target,
                });
                if !expanded.contains(&target) {
                    queue.push_back(target);
                }
            }
        }
        Ok(Self {
            edges,
            truncated: false,
        })
    }
}
