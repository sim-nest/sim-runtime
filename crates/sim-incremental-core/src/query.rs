//! Query registration wrappers.

use std::sync::Arc;

use crate::{FingerprintValue, IncrementalError, Observation, ObservationKind, QueryFrame};

/// The result type returned by query callbacks.
pub type QueryResult<K, V> = Result<V, IncrementalError<K>>;

type QueryBody<K, V> =
    dyn for<'a> Fn(&K, &mut QueryFrame<'a, K, V>) -> QueryResult<K, V> + Send + Sync;

/// A registered query callback.
pub struct Query<K, V> {
    body: Arc<QueryBody<K, V>>,
}

impl<K, V> Clone for Query<K, V> {
    fn clone(&self) -> Self {
        Self {
            body: Arc::clone(&self.body),
        }
    }
}

impl<K, V> Query<K, V> {
    /// Wraps a query callback for registration.
    #[must_use]
    pub fn new<F>(body: F) -> Self
    where
        F: for<'a> Fn(&K, &mut QueryFrame<'a, K, V>) -> QueryResult<K, V> + Send + Sync + 'static,
    {
        Self {
            body: Arc::new(body),
        }
    }

    pub(crate) fn run<'a>(&self, key: &K, frame: &mut QueryFrame<'a, K, V>) -> QueryResult<K, V> {
        (self.body)(key, frame)
    }
}

impl<K, V> QueryFrame<'_, K, V>
where
    K: Ord + Clone,
    V: Clone + FingerprintValue,
{
    /// Reads another query and records a read dependency.
    pub fn read(&mut self, key: K) -> QueryResult<K, V> {
        let value = self.engine.evaluate(key.clone(), self.run)?;
        let observation = self.engine.memo_observation(&key)?;
        self.engine
            .record_observation(self.run, self.observations, observation)?;
        Ok(value)
    }

    /// Records an external observation with the current source revision.
    pub fn observe(&mut self, kind: ObservationKind, key: K) -> Result<(), IncrementalError<K>> {
        let revision = self.engine.source_revision(&key);
        let observation = Observation::new(key, kind, revision, None);
        self.engine
            .record_observation(self.run, self.observations, observation)
    }

    /// Records that a name was missing.
    pub fn observe_missing(&mut self, key: K) -> Result<(), IncrementalError<K>> {
        self.observe(ObservationKind::Missing, key)
    }

    /// Records that a listing was inspected.
    pub fn observe_listing(&mut self, key: K) -> Result<(), IncrementalError<K>> {
        self.observe(ObservationKind::Listing, key)
    }

    /// Records that policy or authority state was inspected.
    pub fn observe_policy(&mut self, key: K) -> Result<(), IncrementalError<K>> {
        self.observe(ObservationKind::Policy, key)
    }

    /// Records that an external backend epoch was inspected.
    pub fn observe_epoch(&mut self, key: K) -> Result<(), IncrementalError<K>> {
        self.observe(ObservationKind::Epoch, key)
    }

    /// Charges additional user-defined work units.
    pub fn charge_work(&mut self, units: usize) -> Result<(), IncrementalError<K>> {
        let key = self.run.root.clone();
        self.engine.charge_work(self.run, &key, units)
    }

    /// Charges additional user-defined output units.
    pub fn charge_output(&mut self, units: usize) -> Result<(), IncrementalError<K>> {
        let key = self.run.root.clone();
        self.engine.charge_output(self.run, &key, units)
    }

    /// Requests cancellation of the current verification run.
    pub fn cancel(&mut self) {
        self.run.cancelled = true;
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn cancellation_requested(&self) -> bool {
        self.run.cancelled
    }
}
