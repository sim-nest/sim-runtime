//! Incremental query engine and query frames.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BudgetKind, ContinuationToken, FingerprintValue, IncrementalError, Observation,
    ObservationKind, Query, QueryBudgets, QueryResult, Revision, ValueFingerprint,
    state::{Node, RunState},
};

/// A dependency-light incremental query engine.
pub struct IncrementalEngine<K, V> {
    pub(crate) queries: BTreeMap<K, Query<K, V>>,
    pub(crate) nodes: BTreeMap<K, Node<K, V>>,
    pub(crate) reverse: BTreeMap<K, BTreeSet<K>>,
    pub(crate) source_revisions: BTreeMap<K, Revision>,
    pub(crate) continuations: BTreeMap<ContinuationToken, K>,
    pub(crate) next_revision: u64,
    pub(crate) next_token: u64,
}

impl<K, V> Default for IncrementalEngine<K, V>
where
    K: Ord + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> IncrementalEngine<K, V>
where
    K: Ord + Clone,
{
    /// Creates an empty incremental query engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queries: BTreeMap::new(),
            nodes: BTreeMap::new(),
            reverse: BTreeMap::new(),
            source_revisions: BTreeMap::new(),
            continuations: BTreeMap::new(),
            next_revision: 1,
            next_token: 1,
        }
    }

    /// Registers or replaces a query callback for `key`.
    pub fn register_query(&mut self, key: K, query: Query<K, V>) {
        self.queries.insert(key.clone(), query);
        self.nodes.entry(key.clone()).or_default().dirty = true;
        self.mark_dirty_cascade(&key);
    }

    /// Registers a query callback function for `key`.
    pub fn register_fn<F>(&mut self, key: K, query: F)
    where
        F: for<'a> Fn(&K, &mut QueryFrame<'a, K, V>) -> QueryResult<K, V> + 'static,
    {
        self.register_query(key, Query::new(query));
    }

    /// Removes a query and marks its dependents dirty.
    pub fn remove_query(&mut self, key: &K) -> bool {
        let removed = self.queries.remove(key).is_some();
        if removed {
            self.mark_dirty_cascade(key);
            self.detach_node(key);
        }
        removed
    }

    /// Advances an external observation stamp and invalidates reverse
    /// dependents.
    pub fn invalidate(&mut self, key: &K) -> Revision {
        let revision = self.alloc_revision();
        self.source_revisions.insert(key.clone(), revision);
        self.mark_dirty_cascade(key);
        revision
    }

    /// Returns the current external observation revision for `key`.
    #[must_use]
    pub fn source_revision(&self, key: &K) -> Revision {
        self.source_revisions
            .get(key)
            .copied()
            .unwrap_or(Revision::ZERO)
    }

    /// Returns dirty memo keys in deterministic order.
    #[must_use]
    pub fn dirty_keys(&self) -> Vec<K> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.dirty)
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Returns the current memo revision for `key`, when present.
    #[must_use]
    pub fn memo_revision(&self, key: &K) -> Option<Revision> {
        self.nodes.get(key).map(|node| node.revision)
    }

    /// Returns the current memo fingerprint for `key`, when present.
    #[must_use]
    pub fn memo_fingerprint(&self, key: &K) -> Option<ValueFingerprint> {
        self.nodes.get(key).and_then(|node| node.fingerprint)
    }

    pub(crate) fn alloc_revision(&mut self) -> Revision {
        let revision = Revision::new(self.next_revision);
        self.next_revision += 1;
        revision
    }

    pub(crate) fn alloc_continuation(&mut self, root: K) -> ContinuationToken {
        let token = ContinuationToken::new(self.next_token);
        self.next_token += 1;
        self.continuations.insert(token, root);
        token
    }
}

impl<K, V> IncrementalEngine<K, V>
where
    K: Ord + Clone,
    V: Clone + FingerprintValue,
{
    /// Verifies a root query using unbounded budgets.
    pub fn verify(&mut self, key: K) -> QueryResult<K, V> {
        self.verify_with_budgets(key, QueryBudgets::default())
    }

    /// Verifies a root query using explicit budgets.
    pub fn verify_with_budgets(&mut self, key: K, budgets: QueryBudgets) -> QueryResult<K, V> {
        let mut run = RunState::new(key.clone(), budgets);
        self.evaluate(key, &mut run)
    }

    /// Resumes the root query represented by a continuation token.
    pub fn resume(&mut self, token: ContinuationToken, budgets: QueryBudgets) -> QueryResult<K, V> {
        let root = self
            .continuations
            .get(&token)
            .cloned()
            .ok_or(IncrementalError::UnknownContinuation { token })?;
        let value = self.verify_with_budgets(root, budgets)?;
        self.continuations.remove(&token);
        Ok(value)
    }

    /// Verifies roots in stable key order using unbounded budgets.
    pub fn verify_many<I>(&mut self, keys: I) -> QueryResult<K, Vec<(K, V)>>
    where
        I: IntoIterator<Item = K>,
    {
        self.verify_many_with_budgets(keys, QueryBudgets::default())
    }

    /// Verifies roots in stable key order using explicit budgets for each root.
    pub fn verify_many_with_budgets<I>(
        &mut self,
        keys: I,
        budgets: QueryBudgets,
    ) -> QueryResult<K, Vec<(K, V)>>
    where
        I: IntoIterator<Item = K>,
    {
        let ordered = keys.into_iter().collect::<BTreeSet<_>>();
        let mut out = Vec::new();
        for key in ordered {
            let value = self.verify_with_budgets(key.clone(), budgets)?;
            out.push((key, value));
        }
        Ok(out)
    }

    pub(crate) fn evaluate(&mut self, key: K, run: &mut RunState<K>) -> QueryResult<K, V> {
        self.check_cancelled(run)?;
        if !self.queries.contains_key(&key) {
            return Err(IncrementalError::UnknownQuery { key });
        }
        if let Some(index) = run.stack.iter().position(|item| item == &key) {
            let mut path = run.stack[index..].to_vec();
            path.push(key);
            return Err(IncrementalError::Cycle { path });
        }
        self.charge_depth(run, &key)?;

        run.stack.push(key.clone());
        let result = self.evaluate_pushed(key, run);
        run.stack.pop();
        result
    }

    fn evaluate_pushed(&mut self, key: K, run: &mut RunState<K>) -> QueryResult<K, V> {
        if self.try_reuse_memo(&key, run)? {
            let value = self
                .nodes
                .get(&key)
                .and_then(|node| node.value.clone())
                .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?;
            return Ok(value);
        }

        self.charge_work(run, &key, 1)?;
        let query = self
            .queries
            .get(&key)
            .cloned()
            .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?;
        let mut observations = Vec::new();
        let value = {
            let mut frame = QueryFrame {
                engine: self,
                run,
                observations: &mut observations,
            };
            query.run(&key, &mut frame)
        };

        let value = value?;
        self.charge_output(run, &key, 1)?;
        self.commit_value(key.clone(), value, observations);
        self.nodes
            .get(&key)
            .and_then(|node| node.value.clone())
            .ok_or(IncrementalError::UnknownQuery { key })
    }

    fn try_reuse_memo(
        &mut self,
        key: &K,
        run: &mut RunState<K>,
    ) -> Result<bool, IncrementalError<K>> {
        let Some(node) = self.nodes.get(key) else {
            return Ok(false);
        };
        if node.value.is_none() {
            return Ok(false);
        }
        let dependencies = node.dependencies.clone();
        let needs_refresh = node.dirty
            || dependencies
                .iter()
                .any(|observation| !self.observation_is_current(observation));
        if needs_refresh {
            for observation in dependencies
                .iter()
                .filter(|observation| matches!(observation.kind(), ObservationKind::Read))
            {
                self.evaluate(observation.key().clone(), run)?;
            }
        }
        if dependencies
            .iter()
            .all(|observation| self.observation_is_current(observation))
        {
            if let Some(node) = self.nodes.get_mut(key) {
                node.dirty = false;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn commit_value(&mut self, key: K, value: V, dependencies: Vec<Observation<K>>) {
        let fingerprint = value.incremental_fingerprint();
        let old_dependencies = self
            .nodes
            .get(&key)
            .map(|node| node.dependencies.clone())
            .unwrap_or_default();
        for observation in old_dependencies {
            if let Some(dependents) = self.reverse.get_mut(observation.key()) {
                dependents.remove(&key);
            }
        }

        let same_value = self
            .nodes
            .get(&key)
            .and_then(|node| node.fingerprint)
            .is_some_and(|old| old == fingerprint);
        let revision = if same_value {
            self.nodes
                .get(&key)
                .map(|node| node.revision)
                .unwrap_or_else(|| self.alloc_revision())
        } else {
            self.alloc_revision()
        };

        for observation in &dependencies {
            self.reverse
                .entry(observation.key().clone())
                .or_default()
                .insert(key.clone());
        }
        self.nodes.insert(
            key,
            Node {
                revision,
                dirty: false,
                value: Some(value),
                fingerprint: Some(fingerprint),
                dependencies,
            },
        );
    }

    fn observation_is_current(&self, observation: &Observation<K>) -> bool {
        match observation.kind() {
            ObservationKind::Read => self.nodes.get(observation.key()).is_some_and(|node| {
                !node.dirty
                    && node.revision == observation.revision()
                    && node.fingerprint == observation.fingerprint()
            }),
            ObservationKind::Missing
            | ObservationKind::Listing
            | ObservationKind::Policy
            | ObservationKind::Epoch
            | ObservationKind::Custom(_) => {
                self.source_revision(observation.key()) == observation.revision()
            }
        }
    }

    pub(crate) fn memo_observation(&self, key: &K) -> Result<Observation<K>, IncrementalError<K>> {
        let node = self
            .nodes
            .get(key)
            .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?;
        let fingerprint = node
            .fingerprint
            .ok_or_else(|| IncrementalError::UnknownQuery { key: key.clone() })?;
        Ok(Observation::read(key.clone(), node.revision, fingerprint))
    }

    pub(crate) fn record_observation(
        &mut self,
        run: &mut RunState<K>,
        observations: &mut Vec<Observation<K>>,
        observation: Observation<K>,
    ) -> Result<(), IncrementalError<K>> {
        self.charge_observation(run, observation.key())?;
        observations.push(observation);
        Ok(())
    }

    pub(crate) fn charge_work(
        &mut self,
        run: &mut RunState<K>,
        key: &K,
        units: usize,
    ) -> Result<(), IncrementalError<K>> {
        self.check_cancelled(run)?;
        if run.work.saturating_add(units) > run.budgets.max_work {
            return Err(self.budget_error(
                run,
                key,
                BudgetKind::Work,
                run.budgets.max_work,
                run.work.saturating_add(units),
            ));
        }
        run.work += units;
        Ok(())
    }

    pub(crate) fn charge_output(
        &mut self,
        run: &mut RunState<K>,
        key: &K,
        units: usize,
    ) -> Result<(), IncrementalError<K>> {
        self.check_cancelled(run)?;
        if run.output.saturating_add(units) > run.budgets.max_output {
            return Err(self.budget_error(
                run,
                key,
                BudgetKind::Output,
                run.budgets.max_output,
                run.output.saturating_add(units),
            ));
        }
        run.output += units;
        Ok(())
    }

    fn charge_depth(&mut self, run: &mut RunState<K>, key: &K) -> Result<(), IncrementalError<K>> {
        if run.stack.len().saturating_add(1) > run.budgets.max_depth {
            return Err(self.budget_error(
                run,
                key,
                BudgetKind::Depth,
                run.budgets.max_depth,
                run.stack.len().saturating_add(1),
            ));
        }
        Ok(())
    }

    fn charge_observation(
        &mut self,
        run: &mut RunState<K>,
        key: &K,
    ) -> Result<(), IncrementalError<K>> {
        if run.observations.saturating_add(1) > run.budgets.max_observations {
            return Err(self.budget_error(
                run,
                key,
                BudgetKind::Observations,
                run.budgets.max_observations,
                run.observations.saturating_add(1),
            ));
        }
        run.observations += 1;
        Ok(())
    }

    fn check_cancelled(&self, run: &RunState<K>) -> Result<(), IncrementalError<K>> {
        if run.cancelled {
            Err(IncrementalError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn budget_error(
        &mut self,
        run: &RunState<K>,
        _key: &K,
        kind: BudgetKind,
        limit: usize,
        consumed: usize,
    ) -> IncrementalError<K> {
        let continuation = Some(self.alloc_continuation(run.root.clone()));
        IncrementalError::BudgetExceeded {
            kind,
            limit,
            consumed,
            continuation,
        }
    }
}

impl<K, V> IncrementalEngine<K, V>
where
    K: Ord + Clone,
{
    fn mark_dirty_cascade(&mut self, key: &K) {
        let mut pending = BTreeSet::from([key.clone()]);
        let mut seen = BTreeSet::new();
        while let Some(next) = pending.iter().next().cloned() {
            pending.remove(&next);
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Some(node) = self.nodes.get_mut(&next) {
                node.dirty = true;
            }
            if let Some(dependents) = self.reverse.get(&next) {
                pending.extend(dependents.iter().cloned());
            }
        }
    }

    fn detach_node(&mut self, key: &K) {
        let Some(node) = self.nodes.remove(key) else {
            return;
        };
        for observation in node.dependencies {
            if let Some(dependents) = self.reverse.get_mut(observation.key()) {
                dependents.remove(key);
            }
        }
        self.reverse.remove(key);
    }
}

/// Execution context handed to a query callback.
pub struct QueryFrame<'a, K, V> {
    pub(crate) engine: &'a mut IncrementalEngine<K, V>,
    pub(crate) run: &'a mut RunState<K>,
    pub(crate) observations: &'a mut Vec<Observation<K>>,
}
