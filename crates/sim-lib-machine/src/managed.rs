use sim_lib_control::WorkLimit;
use sim_lib_mutation::ManagedId;

/// Projects live machine storage into the shared managed-object identity space.
///
/// Implementations enumerate identities only. Reclamation policy remains outside
/// the machine, so the same suspended state can be inspected by any tracing or
/// retention implementation built over `sim-lib-mutation`.
pub trait ManagedRootSource {
    /// Visits roots in a stable, source-defined order.
    ///
    /// Returns `false` immediately when the visitor refuses further work.
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool;
}

impl ManagedRootSource for ManagedId {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        visit(*self)
    }
}

impl<T: ManagedRootSource> ManagedRootSource for Option<T> {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        if let Some(value) = self {
            value.visit_managed_roots(visit)
        } else {
            true
        }
    }
}

impl<T: ManagedRootSource> ManagedRootSource for Vec<T> {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        for value in self {
            if !value.visit_managed_roots(visit) {
                return false;
            }
        }
        true
    }
}

impl ManagedRootSource for () {
    fn visit_managed_roots(&self, _visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        true
    }
}

/// Deterministic refusal to materialize a complete root snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootScanError {
    /// The next root would exceed the caller-supplied work budget.
    BudgetExhausted {
        /// Roots admitted before the refusal.
        visited: usize,
        /// Maximum roots admitted for this scan.
        limit: usize,
    },
}

/// A complete, ordered view of roots visible at one machine safepoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootSnapshot {
    roots: Vec<ManagedId>,
}

impl RootSnapshot {
    /// Materializes all roots atomically or returns exact budget exhaustion.
    pub fn scan(source: &impl ManagedRootSource, budget: WorkLimit) -> Result<Self, RootScanError> {
        let mut roots = Vec::new();
        let complete = source.visit_managed_roots(&mut |root| {
            if roots.len() == budget.0 {
                return false;
            }
            roots.push(root);
            true
        });
        if !complete {
            Err(RootScanError::BudgetExhausted {
                visited: budget.0,
                limit: budget.0,
            })
        } else {
            Ok(Self { roots })
        }
    }

    /// Returns roots in machine storage order.
    pub fn roots(&self) -> &[ManagedId] {
        &self.roots
    }
}
