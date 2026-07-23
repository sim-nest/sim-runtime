//! Internal memo and run-state records.

use crate::{Observation, QueryBudgets, Revision, ValueFingerprint};

#[derive(Clone, Debug)]
pub(crate) struct Node<K, V> {
    pub(crate) revision: Revision,
    pub(crate) dirty: bool,
    pub(crate) value: Option<V>,
    pub(crate) fingerprint: Option<ValueFingerprint>,
    pub(crate) dependencies: Vec<Observation<K>>,
}

impl<K, V> Default for Node<K, V> {
    fn default() -> Self {
        Self {
            revision: Revision::ZERO,
            dirty: true,
            value: None,
            fingerprint: None,
            dependencies: Vec::new(),
        }
    }
}

pub(crate) struct RunState<K> {
    pub(crate) root: K,
    pub(crate) budgets: QueryBudgets,
    pub(crate) stack: Vec<K>,
    pub(crate) work: usize,
    pub(crate) observations: usize,
    pub(crate) output: usize,
    pub(crate) cancelled: bool,
}

impl<K> RunState<K> {
    pub(crate) fn new(root: K, budgets: QueryBudgets) -> Self {
        Self {
            root,
            budgets,
            stack: Vec::new(),
            work: 0,
            observations: 0,
            output: 0,
            cancelled: false,
        }
    }
}
