/// A role-bearing managed object with stable, ordered strong edges.
///
/// Edge storage and identity allocation are deliberately private. Callers can
/// mutate the graph only through checked operations, so identities are never
/// reused and compare-and-mutate failures leave the node unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedNode<R> {
    role: ManagedRole<R>,
    edges: EdgeAllocator,
    limits: EdgeLimits,
    strong: BTreeMap<EdgeId, ManagedId>,
    weak: BTreeMap<EdgeId, ManagedId>,
    ephemerons: BTreeMap<EdgeId, (ManagedId, ManagedId)>,
}
/// A managed object carrying caller-owned role evidence outside graph policy.
pub trait RoleBearingManagedObject: ManagedObject {
    /// Role label type chosen by the object owner.
    type Role;
    /// Borrows the current audit role.
    fn managed_role(&self) -> &Self::Role;
    /// Replaces audit role evidence without changing the managed graph.
    fn replace_managed_role(&mut self, role: Self::Role) -> Self::Role;
}

impl<R> ManagedNode<R> {
    /// Constructs an empty node carrying caller-owned role evidence.
    pub const fn new(role: R) -> Self {
        Self::with_edge_limits(role, EdgeLimits::DEFAULT)
    }

    /// Constructs an empty node with explicit total and per-kind hard caps.
    pub const fn with_edge_limits(role: R, limits: EdgeLimits) -> Self {
        Self {
            role: ManagedRole::new(role),
            edges: EdgeAllocator::new(),
            limits,
            strong: BTreeMap::new(),
            weak: BTreeMap::new(),
            ephemerons: BTreeMap::new(),
        }
    }

    /// Returns an allocation-ordered copy of every live outgoing edge.
    pub fn edge_snapshot(&self) -> Vec<EdgeSnapshot> {
        let mut result = self
            .strong
            .iter()
            .map(|(&edge, &target)| EdgeSnapshot::Strong { edge, target })
            .chain(
                self.weak
                    .iter()
                    .map(|(&edge, &target)| EdgeSnapshot::Weak { edge, target }),
            )
            .chain(
                self.ephemerons
                    .iter()
                    .map(|(&edge, &(key, value))| EdgeSnapshot::Ephemeron { edge, key, value }),
            )
            .collect::<Vec<_>>();
        result.sort_unstable_by_key(|entry| entry.id());
        result
    }

    fn edge_kind(&self, edge: EdgeId) -> Option<EdgeKind> {
        self.strong
            .contains_key(&edge)
            .then_some(EdgeKind::Strong)
            .or_else(|| self.weak.contains_key(&edge).then_some(EdgeKind::Weak))
            .or_else(|| {
                self.ephemerons
                    .contains_key(&edge)
                    .then_some(EdgeKind::Ephemeron)
            })
    }

    fn admit(&self, kind: EdgeKind) -> Result<(), EdgeAllocationError> {
        let total = self.strong.len() + self.weak.len() + self.ephemerons.len();
        if total >= self.limits.total() {
            return Err(EdgeAllocationError::CapacityExceeded {
                kind,
                cap: self.limits.total(),
            });
        }
        let count = match kind {
            EdgeKind::Strong => self.strong.len(),
            EdgeKind::Weak => self.weak.len(),
            EdgeKind::Ephemeron => self.ephemerons.len(),
        };
        let cap = self.limits.for_kind(kind);
        if count >= cap {
            return Err(EdgeAllocationError::CapacityExceeded { kind, cap });
        }
        Ok(())
    }

    /// Borrows the node's role evidence.
    pub const fn role(&self) -> &R {
        self.role.role()
    }

    /// Replaces the role without changing the managed graph.
    pub fn replace_role(&mut self, role: R) -> R {
        self.role.replace_role(role)
    }

    /// Inserts a strong edge and returns its stable identity.
    pub fn insert_strong(&mut self, target: ManagedId) -> Result<EdgeId, StrongEdgeMutationError> {
        self.admit(EdgeKind::Strong)?;
        let edge = self.edges.allocate(EdgeKind::Strong)?.id();
        let previous = self.strong.insert(edge, target);
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces a strong target only if it still equals `expected`.
    pub fn replace_strong(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
        replacement: ManagedId,
    ) -> Result<(), StrongEdgeMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Strong)
        {
            return Err(StrongEdgeMutationError::WrongKind { edge, actual });
        }
        let target = self
            .strong
            .get_mut(&edge)
            .ok_or(StrongEdgeMutationError::UnknownEdge(edge))?;
        if *target != expected {
            return Err(StrongEdgeMutationError::TargetChanged {
                expected,
                actual: *target,
            });
        }
        *target = replacement;
        Ok(())
    }

    /// Removes a strong edge only if it still equals `expected`.
    pub fn remove_strong(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
    ) -> Result<ManagedId, StrongEdgeMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Strong)
        {
            return Err(StrongEdgeMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .strong
            .get(&edge)
            .copied()
            .ok_or(StrongEdgeMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(StrongEdgeMutationError::TargetChanged { expected, actual });
        }
        Ok(self
            .strong
            .remove(&edge)
            .expect("edge checked immediately before removal"))
    }

    /// Inserts a weak edge and returns its stable identity.
    pub fn insert_weak(&mut self, target: ManagedId) -> Result<EdgeId, WeakEdgeMutationError> {
        self.admit(EdgeKind::Weak)?;
        let edge = self.edges.allocate(EdgeKind::Weak)?.id();
        let previous = self.weak.insert(edge, target);
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces a weak target only if it still equals `expected`.
    pub fn replace_weak(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
        replacement: ManagedId,
    ) -> Result<(), WeakEdgeMutationError> {
        if let Some(actual) = self.edge_kind(edge).filter(|kind| *kind != EdgeKind::Weak) {
            return Err(WeakEdgeMutationError::WrongKind { edge, actual });
        }
        let target = self
            .weak
            .get_mut(&edge)
            .ok_or(WeakEdgeMutationError::UnknownEdge(edge))?;
        if *target != expected {
            return Err(WeakEdgeMutationError::TargetChanged {
                expected,
                actual: *target,
            });
        }
        *target = replacement;
        Ok(())
    }

    /// Removes a weak edge only if it still equals `expected`.
    pub fn remove_weak(
        &mut self,
        edge: EdgeId,
        expected: ManagedId,
    ) -> Result<ManagedId, WeakEdgeMutationError> {
        if let Some(actual) = self.edge_kind(edge).filter(|kind| *kind != EdgeKind::Weak) {
            return Err(WeakEdgeMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .weak
            .get(&edge)
            .copied()
            .ok_or(WeakEdgeMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(WeakEdgeMutationError::TargetChanged { expected, actual });
        }
        Ok(self
            .weak
            .remove(&edge)
            .expect("edge checked immediately before removal"))
    }

    /// Inserts an ephemeron entry and returns its stable identity.
    pub fn insert_ephemeron(
        &mut self,
        key: ManagedId,
        value: ManagedId,
    ) -> Result<EdgeId, EphemeronMutationError> {
        self.admit(EdgeKind::Ephemeron)?;
        let edge = self.edges.allocate(EdgeKind::Ephemeron)?.id();
        let previous = self.ephemerons.insert(edge, (key, value));
        debug_assert!(previous.is_none(), "fresh edge identity must be vacant");
        Ok(edge)
    }

    /// Replaces an ephemeron only if it still contains the expected pair.
    pub fn replace_ephemeron(
        &mut self,
        edge: EdgeId,
        expected: (ManagedId, ManagedId),
        replacement: (ManagedId, ManagedId),
    ) -> Result<(), EphemeronMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Ephemeron)
        {
            return Err(EphemeronMutationError::WrongKind { edge, actual });
        }
        let entry = self
            .ephemerons
            .get_mut(&edge)
            .ok_or(EphemeronMutationError::UnknownEdge(edge))?;
        if *entry != expected {
            return Err(EphemeronMutationError::EntryChanged {
                expected_key: expected.0,
                expected_value: expected.1,
                actual_key: entry.0,
                actual_value: entry.1,
            });
        }
        *entry = replacement;
        Ok(())
    }

    /// Removes an ephemeron only if it still contains the expected pair.
    pub fn remove_ephemeron(
        &mut self,
        edge: EdgeId,
        expected: (ManagedId, ManagedId),
    ) -> Result<(ManagedId, ManagedId), EphemeronMutationError> {
        if let Some(actual) = self
            .edge_kind(edge)
            .filter(|kind| *kind != EdgeKind::Ephemeron)
        {
            return Err(EphemeronMutationError::WrongKind { edge, actual });
        }
        let actual = self
            .ephemerons
            .get(&edge)
            .copied()
            .ok_or(EphemeronMutationError::UnknownEdge(edge))?;
        if actual != expected {
            return Err(EphemeronMutationError::EntryChanged {
                expected_key: expected.0,
                expected_value: expected.1,
                actual_key: actual.0,
                actual_value: actual.1,
            });
        }
        Ok(self
            .ephemerons
            .remove(&edge)
            .expect("entry checked immediately before removal"))
    }
}

impl<R> ManagedObject for ManagedNode<R> {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        let mut edges = self
            .strong
            .iter()
            .map(|(&edge, &target)| (edge, EdgeKind::Strong, target, target))
            .chain(
                self.weak
                    .iter()
                    .map(|(&edge, &target)| (edge, EdgeKind::Weak, target, target)),
            )
            .chain(
                self.ephemerons
                    .iter()
                    .map(|(&edge, &(key, value))| (edge, EdgeKind::Ephemeron, key, value)),
            )
            .collect::<Vec<_>>();
        edges.sort_unstable_by_key(|entry| entry.0);
        for (edge, kind, first, second) in edges {
            match kind {
                EdgeKind::Strong => visitor.strong(edge, first),
                EdgeKind::Weak => visitor.weak(edge, first),
                EdgeKind::Ephemeron => visitor.ephemeron(edge, first, second),
            }
        }
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        if self.weak.get(&edge) != Some(&expected) {
            return false;
        }
        self.weak.remove(&edge).is_some()
    }

    fn clear_ephemeron_edge(
        &mut self,
        edge: EdgeId,
        expected_key: ManagedId,
        expected_value: ManagedId,
    ) -> bool {
        if self.ephemerons.get(&edge) != Some(&(expected_key, expected_value)) {
            return false;
        }
        self.ephemerons.remove(&edge).is_some()
    }
}

impl<R> RoleBearingManagedObject for ManagedNode<R> {
    type Role = R;

    fn managed_role(&self) -> &Self::Role {
        self.role()
    }

    fn replace_managed_role(&mut self, role: Self::Role) -> Self::Role {
        self.replace_role(role)
    }
}
