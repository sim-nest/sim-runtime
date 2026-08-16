/// Bounded storage for managed objects, independent of language and collector policy.
pub struct ManagedArena<T> {
    policy: HardCappedRetainPolicy,
    next_id: u64,
    next_root: u64,
    next_safepoint: u64,
    mutation_epoch: u64,
    objects: BTreeMap<ManagedId, T>,
    roots: BTreeMap<RootId, ManagedId>,
    kept_alive: BTreeMap<ManagedId, u64>,
}

impl<T> ManagedArena<T> {
    /// Creates an empty arena using the hard-capped retain policy.
    pub fn new(policy: HardCappedRetainPolicy) -> Self {
        Self {
            policy,
            next_id: 0,
            next_root: 0,
            next_safepoint: 0,
            mutation_epoch: 0,
            objects: BTreeMap::new(),
            roots: BTreeMap::new(),
            kept_alive: BTreeMap::new(),
        }
    }

    /// Returns the tracing contract version.
    pub const fn trace_contract_version(&self) -> TraceContractVersion {
        TraceContractVersion::V1
    }

    /// Returns the number of live objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Reports whether the arena contains no objects.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Returns the epoch advanced by every graph-affecting arena mutation.
    pub const fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    fn advance_mutation_epoch(&mut self) -> Result<(), ArenaError> {
        self.mutation_epoch = self
            .mutation_epoch
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        Ok(())
    }

    /// Allocates atomically after checking the cap and identity space.
    pub fn allocate(&mut self, object: T) -> Result<ManagedHandle, ArenaError> {
        if self.objects.len() >= self.policy.max_objects {
            return Err(ArenaError::CapacityExceeded {
                cap: self.policy.max_objects,
            });
        }
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let id = ManagedId(self.next_id);
        self.advance_mutation_epoch()?;
        self.objects.insert(id, object);
        self.next_id = next;
        Ok(ManagedHandle { id })
    }

    /// Returns a shared object reference, refusing stale handles.
    pub fn get(&self, handle: ManagedHandle) -> Result<&T, ArenaError> {
        self.objects
            .get(&handle.id)
            .ok_or(ArenaError::StaleHandle(handle.id))
    }

    /// Returns a mutable object reference, refusing stale handles.
    pub fn get_mut(&mut self, handle: ManagedHandle) -> Result<&mut T, ArenaError> {
        if !self.objects.contains_key(&handle.id) {
            return Err(ArenaError::StaleHandle(handle.id));
        }
        self.advance_mutation_epoch()?;
        Ok(self
            .objects
            .get_mut(&handle.id)
            .expect("validated managed id"))
    }

    /// Upgrades a weak handle only while its object remains live.
    pub fn upgrade(&mut self, weak: WeakHandle) -> Result<ManagedHandle, ArenaError> {
        if !self.objects.contains_key(&weak.id) {
            return Err(ArenaError::StaleHandle(weak.id));
        }
        self.kept_alive.insert(weak.id, self.mutation_epoch);
        Ok(ManagedHandle { id: weak.id })
    }

    /// Resolves a tracing identity to a live handle for collector operations.
    pub fn handle(&self, id: ManagedId) -> Result<ManagedHandle, ArenaError> {
        self.objects
            .contains_key(&id)
            .then_some(ManagedHandle { id })
            .ok_or(ArenaError::StaleHandle(id))
    }

    /// Registers a root after validating the handle.
    pub fn root(&mut self, handle: ManagedHandle) -> Result<RootedHandle, ArenaError> {
        self.get(handle)?;
        let next = self
            .next_root
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let root = RootId(self.next_root);
        self.advance_mutation_epoch()?;
        self.roots.insert(root, handle.id);
        self.next_root = next;
        Ok(RootedHandle { root, handle })
    }

    /// Releases exactly one matching root registration.
    pub fn release_root(&mut self, rooted: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        match self.roots.get(&rooted.root) {
            Some(id) if *id == rooted.handle.id => {
                self.advance_mutation_epoch()?;
                self.roots.remove(&rooted.root);
                Ok(rooted.handle)
            }
            _ => Err(ArenaError::StaleRoot(rooted.root)),
        }
    }

    /// Removes an unrooted object, making all handles to it stale.
    pub fn remove(&mut self, handle: ManagedHandle) -> Result<T, ArenaError> {
        if self.roots.values().any(|id| *id == handle.id) {
            return Err(ArenaError::ObjectRooted(handle.id));
        }
        if !self.objects.contains_key(&handle.id) {
            return Err(ArenaError::StaleHandle(handle.id));
        }
        self.advance_mutation_epoch()?;
        let removed = self
            .objects
            .remove(&handle.id)
            .expect("validated managed id");
        Ok(removed)
    }

    /// Clears a weak edge through the owning object's at-most-once operation.
    pub fn clear_weak_edge(
        &mut self,
        owner: ManagedHandle,
        edge: EdgeId,
        expected: WeakHandle,
    ) -> Result<bool, ArenaError>
    where
        T: ManagedObject,
    {
        if !self.objects.contains_key(&owner.id) {
            return Err(ArenaError::StaleHandle(owner.id));
        }
        self.advance_mutation_epoch()?;
        let cleared = self
            .objects
            .get_mut(&owner.id)
            .expect("validated managed id")
            .clear_weak_edge(edge, expected.id);
        Ok(cleared)
    }

    /// Atomically removes an allocation-ordered set selected from `expected_epoch`.
    ///
    /// Every identity and root condition is checked before the first slot changes.
    pub fn sweep_at_epoch(
        &mut self,
        expected_epoch: u64,
        objects: &[ManagedId],
    ) -> Result<Vec<ManagedId>, ArenaError> {
        if self.mutation_epoch != expected_epoch {
            return Err(ArenaError::MutationEpochChanged {
                expected: expected_epoch,
                actual: self.mutation_epoch,
            });
        }
        for id in objects {
            if !self.objects.contains_key(id) {
                return Err(ArenaError::StaleHandle(*id));
            }
            if self.roots.values().any(|rooted| rooted == id) {
                return Err(ArenaError::ObjectRooted(*id));
            }
        }
        if !objects.is_empty() {
            self.advance_mutation_epoch()?;
        }
        for id in objects {
            self.objects.remove(id);
        }
        Ok(objects.to_vec())
    }

    /// Applies a collector plan atomically at `expected_epoch`.
    ///
    /// Kept-alive objects from that epoch are retained. Weak and ephemeron
    /// entries are cleared before unreachable objects are removed, and every
    /// conditional clear is intrinsically at most once.
    pub fn apply_collection_at_epoch(
        &mut self,
        expected_epoch: u64,
        weak: &[(ManagedId, EdgeId, ManagedId)],
        ephemerons: &[(ManagedId, EdgeId, ManagedId, ManagedId)],
        swept: &[ManagedId],
    ) -> Result<CollectionMutationReceipt, ArenaError>
    where
        T: ManagedObject,
    {
        if self.mutation_epoch != expected_epoch {
            return Err(ArenaError::MutationEpochChanged {
                expected: expected_epoch,
                actual: self.mutation_epoch,
            });
        }
        let kept = self
            .kept_alive
            .iter()
            .filter_map(|(id, epoch)| (*epoch == expected_epoch).then_some(*id))
            .collect::<std::collections::BTreeSet<_>>();
        let actual_swept = swept
            .iter()
            .copied()
            .filter(|id| !kept.contains(id))
            .collect::<Vec<_>>();
        for id in &actual_swept {
            if !self.objects.contains_key(id) {
                return Err(ArenaError::StaleHandle(*id));
            }
            if self.roots.values().any(|rooted| rooted == id) {
                return Err(ArenaError::ObjectRooted(*id));
            }
        }
        if !weak.is_empty() || !ephemerons.is_empty() || !actual_swept.is_empty() {
            self.advance_mutation_epoch()?;
        }
        let mut cleared_weak = Vec::new();
        for &(owner, edge, target) in weak {
            if let Some(object) = self.objects.get_mut(&owner)
                && object.clear_weak_edge(edge, target)
            {
                cleared_weak.push((owner, edge));
            }
        }
        let mut cleared_ephemerons = Vec::new();
        for &(owner, edge, key, value) in ephemerons {
            if let Some(object) = self.objects.get_mut(&owner)
                && object.clear_ephemeron_edge(edge, key, value)
            {
                cleared_ephemerons.push((owner, edge));
            }
        }
        for id in &actual_swept {
            self.objects.remove(id);
        }
        self.kept_alive
            .retain(|id, epoch| self.objects.contains_key(id) && *epoch != expected_epoch);
        Ok(CollectionMutationReceipt {
            cleared_weak,
            cleared_ephemerons,
            swept: actual_swept,
        })
    }

    /// Runs a read-only tracing callback at a deterministic safepoint.
    pub fn safepoint<R>(
        &mut self,
        trace: impl FnOnce(&TraceSnapshot<'_, T>) -> R,
    ) -> Result<(R, SafepointReceipt), ArenaError>
    where
        T: ManagedObject,
    {
        let next = self
            .next_safepoint
            .checked_add(1)
            .ok_or(ArenaError::IdentityExhausted)?;
        let roots = self.roots.values().copied().collect::<Vec<_>>();
        let snapshot = TraceSnapshot {
            roots: roots.clone(),
            kept_alive: self
                .kept_alive
                .iter()
                .filter_map(|(id, epoch)| (*epoch == self.mutation_epoch).then_some(*id))
                .collect(),
            objects: &self.objects,
            mutation_epoch: self.mutation_epoch,
        };
        let result = trace(&snapshot);
        let receipt = SafepointReceipt {
            sequence: self.next_safepoint,
            roots,
            objects: self.objects.keys().copied().collect(),
        };
        self.next_safepoint = next;
        Ok((result, receipt))
    }

    /// Tears down all storage and roots, returning allocation-ordered evidence.
    pub fn teardown(&mut self) -> TeardownReceipt {
        let receipt = TeardownReceipt {
            objects: self.objects.keys().copied().collect(),
            roots: self.roots.keys().copied().collect(),
        };
        if !self.objects.is_empty() || !self.roots.is_empty() {
            self.mutation_epoch = self.mutation_epoch.saturating_add(1);
        }
        self.objects.clear();
        self.roots.clear();
        self.kept_alive.clear();
        receipt
    }
}

impl<T: RoleBearingManagedObject> ManagedArena<T> {
    /// Replaces caller-owned role evidence without advancing the graph epoch.
    pub fn replace_role(
        &mut self,
        handle: ManagedHandle,
        role: T::Role,
    ) -> Result<T::Role, ArenaError> {
        self.objects
            .get_mut(&handle.id)
            .map(|object| object.replace_managed_role(role))
            .ok_or(ArenaError::StaleHandle(handle.id))
    }

    /// Projects optional owner-role evidence at a read-only safepoint.
    ///
    /// Admission is checked before invoking `role`, and rows follow managed id
    /// order. The projection observes objects but is not visible to tracing or
    /// collector policy.
    pub fn project_roles(
        &mut self,
        limit: usize,
    ) -> Result<RoleProjectionReceipt<T::Role>, RoleProjectionError>
    where
        T::Role: Clone,
    {
        let (roles, safepoint) = self.safepoint(|snapshot| {
            let required = snapshot.objects.len();
            if required > limit {
                return Err(RoleProjectionError::Limit { limit, required });
            }
            let roles = snapshot
                .objects
                .iter()
                .map(|(&id, object)| (id, object.managed_role().clone()))
                .collect();
            Ok((snapshot.mutation_epoch(), roles))
        })?;
        let (mutation_epoch, roles) = roles?;
        Ok(RoleProjectionReceipt {
            safepoint,
            mutation_epoch,
            roles,
        })
    }
}
