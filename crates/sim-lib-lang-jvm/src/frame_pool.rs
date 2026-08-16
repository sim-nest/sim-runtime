//! Bounded reuse of JVM activation storage over the shared machine organs.

use std::sync::{Arc, Mutex, MutexGuard};

use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_machine::{ManagedRootSource, SlotFile, UnitStack};
use sim_lib_mutation::ManagedId;

use crate::JvmValueWidth;

/// Loader- or runtime-owned policy for retaining completed JVM frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JvmFramePoolPolicy {
    /// Maximum number of sanitized frame records retained.
    pub frames: usize,
    /// Greatest local-slot file eligible for retention.
    pub slots: usize,
    /// Greatest operand depth eligible for retention.
    pub operands: usize,
}

/// A JVM activation record composed from the shared machine storage organs.
pub struct JvmFrameRecord {
    locals: SlotFile<JvmValueWidth>,
    operands: UnitStack<JvmValueWidth>,
    operand_limit: usize,
    prepared_roots: Vec<ManagedId>,
    dirty_locals: Vec<bool>,
    operands_dirty: bool,
    root_map_certain: bool,
}

impl JvmFrameRecord {
    fn new(slots: usize, operands: usize) -> Self {
        Self {
            locals: SlotFile::new(AdmissionLimit(slots)),
            operands: UnitStack::new(WorkLimit(operands)),
            operand_limit: operands,
            prepared_roots: Vec::new(),
            dirty_locals: vec![true; slots],
            operands_dirty: true,
            root_map_certain: false,
        }
    }

    fn clear(&mut self) {
        for slot in 0..self.locals.limit() {
            let _ = self.locals.release(slot);
        }
        self.operands.clear();
        self.prepared_roots.clear();
        self.dirty_locals.fill(false);
        self.operands_dirty = false;
        self.root_map_certain = true;
    }

    /// Returns the frame's bounded local-slot file.
    pub fn locals(&self) -> &SlotFile<JvmValueWidth> {
        &self.locals
    }

    /// Returns the mutable bounded local-slot file.
    pub fn locals_mut(&mut self) -> &mut SlotFile<JvmValueWidth> {
        // SlotFile deliberately exposes whole-span replacement. Until its mutation is
        // observed at a narrower boundary, every local is conservatively suspect.
        self.dirty_locals.fill(true);
        self.root_map_certain = false;
        &mut self.locals
    }

    /// Returns the frame's bounded operand stack.
    pub fn operands(&self) -> &UnitStack<JvmValueWidth> {
        &self.operands
    }

    /// Returns the mutable bounded operand stack.
    pub fn operands_mut(&mut self) -> &mut UnitStack<JvmValueWidth> {
        self.operands_dirty = true;
        self.root_map_certain = false;
        &mut self.operands
    }

    /// Returns the admitted operand depth for this record.
    pub const fn operand_limit(&self) -> usize {
        self.operand_limit
    }

    /// Returns the root set for a test or collection safepoint.
    ///
    /// A clean prepared map avoids walking frame storage. Any dirty or uncertain
    /// mutation ledger is repaired from the complete enumerator before the map is
    /// trusted. The complete enumerator remains the authority in all cases.
    pub fn safepoint_roots(&mut self) -> &[ManagedId] {
        if !self.root_map_certain
            || self.operands_dirty
            || self.dirty_locals.iter().any(|dirty| *dirty)
        {
            self.rebuild_prepared_roots();
        }

        #[cfg(test)]
        self.assert_safepoint_equivalence();

        &self.prepared_roots
    }

    fn rebuild_prepared_roots(&mut self) {
        let mut roots = Vec::new();
        let complete = self.visit_managed_roots(&mut |root| {
            roots.push(root);
            true
        });
        debug_assert!(complete, "unbounded root collection cannot be refused");
        self.prepared_roots = roots;
        self.dirty_locals.fill(false);
        self.operands_dirty = false;
        self.root_map_certain = true;
    }

    #[cfg(test)]
    fn assert_safepoint_equivalence(&self) {
        let mut full = Vec::new();
        assert!(self.visit_managed_roots(&mut |root| {
            full.push(root);
            true
        }));
        assert_eq!(
            self.prepared_roots, full,
            "prepared JVM root map diverged from the complete enumerator"
        );
    }
}

impl ManagedRootSource for JvmFrameRecord {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        let mut complete = true;
        self.locals
            .visit_values(|value| complete = complete && value.visit_managed_roots(visit));
        if !complete {
            return false;
        }
        self.operands
            .visit_values(|value| complete = complete && value.visit_managed_roots(visit));
        complete
    }
}

struct PoolState {
    retained: Vec<JvmFrameRecord>,
}

struct PoolInner {
    policy: JvmFramePoolPolicy,
    state: Mutex<PoolState>,
}

/// A bounded pool whose records are visible to the complete managed-root enumerator.
#[derive(Clone)]
pub struct JvmFramePool {
    inner: Arc<PoolInner>,
}

impl JvmFramePool {
    /// Creates an empty pool with explicit record and retained-capacity caps.
    pub fn new(policy: JvmFramePoolPolicy) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                policy,
                state: Mutex::new(PoolState {
                    retained: Vec::new(),
                }),
            }),
        }
    }

    /// Acquires one exclusively owned frame, reusing only an exact sanitized shape.
    pub fn acquire(&self, slots: usize, operands: usize) -> JvmFrameLease {
        let frame = {
            let mut state = self.state();
            state
                .retained
                .iter()
                .position(|frame| frame.locals.limit() == slots && frame.operand_limit == operands)
                .map(|index| state.retained.swap_remove(index))
        }
        .unwrap_or_else(|| JvmFrameRecord::new(slots, operands));
        JvmFrameLease {
            pool: self.clone(),
            frame: Some(frame),
        }
    }

    /// Returns the current number of sanitized retained records.
    pub fn retained_frames(&self) -> usize {
        self.state().retained.len()
    }

    fn state(&self) -> MutexGuard<'_, PoolState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn recycle(&self, mut frame: JvmFrameRecord) {
        frame.clear();
        let policy = self.inner.policy;
        if frame.locals.limit() > policy.slots || frame.operand_limit > policy.operands {
            return;
        }
        let mut state = self.state();
        if state.retained.len() < policy.frames {
            state.retained.push(frame);
        }
    }
}

impl ManagedRootSource for JvmFramePool {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        for frame in &self.state().retained {
            if !frame.visit_managed_roots(visit) {
                return false;
            }
        }
        true
    }
}

/// Exclusive ownership of one live or suspended JVM frame.
///
/// Dropping a lease does not recycle it. A caller must prove normal completion
/// by calling [`Self::complete`]; this makes unwinding and abandoned interruption
/// fail closed.
pub struct JvmFrameLease {
    pool: JvmFramePool,
    frame: Option<JvmFrameRecord>,
}

impl JvmFrameLease {
    /// Returns the exclusively owned frame record.
    pub fn frame(&self) -> &JvmFrameRecord {
        self.frame.as_ref().expect("live frame lease")
    }

    /// Returns the exclusively owned mutable frame record.
    pub fn frame_mut(&mut self) -> &mut JvmFrameRecord {
        self.frame.as_mut().expect("live frame lease")
    }

    /// Retains this live frame across an interruption without exposing it to the pool.
    pub fn interrupt(self) -> InterruptedJvmFrame {
        InterruptedJvmFrame { lease: self }
    }

    /// Sanitizes and conditionally retains a normally returned frame.
    pub fn complete(mut self) {
        let frame = self.frame.take().expect("live frame lease");
        self.pool.recycle(frame);
    }
}

impl ManagedRootSource for JvmFrameLease {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        self.frame().visit_managed_roots(visit)
    }
}

/// A suspended activation that remains exclusively owned until resumed or discarded.
pub struct InterruptedJvmFrame {
    lease: JvmFrameLease,
}

impl InterruptedJvmFrame {
    /// Returns the suspended frame for root enumeration at the interruption boundary.
    pub fn frame(&self) -> &JvmFrameRecord {
        self.lease.frame()
    }

    /// Resumes exclusive execution of the same activation.
    pub fn resume(self) -> JvmFrameLease {
        self.lease
    }
}

impl ManagedRootSource for InterruptedJvmFrame {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        self.lease.visit_managed_roots(visit)
    }
}

#[cfg(test)]
mod tests {
    use sim_lib_mutation::{HardCappedRetainPolicy, ManagedArena, ManagedNode};

    use super::*;
    use crate::{JvmReference, JvmValue};

    fn handles(count: usize) -> Vec<sim_lib_mutation::ManagedHandle> {
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(count).unwrap());
        (0..count)
            .map(|_| arena.allocate(ManagedNode::new(())).unwrap())
            .collect()
    }

    #[test]
    fn every_test_safepoint_checks_prepared_roots_against_the_full_enumerator() {
        let handles = handles(2);
        let mut frame = JvmFrameRecord::new(2, 2);
        frame
            .locals_mut()
            .store(0, JvmValue::Reference(JvmReference::managed(handles[0])))
            .unwrap();
        frame
            .operands_mut()
            .push(JvmValue::Reference(JvmReference::managed(handles[1])))
            .unwrap();

        assert_eq!(frame.safepoint_roots(), &[handles[0].id(), handles[1].id()]);
        assert_eq!(
            frame.safepoint_roots(),
            &[handles[0].id(), handles[1].id()],
            "a clean safepoint reuses the checked prepared map"
        );
    }

    #[test]
    fn stale_dirty_ledger_falls_back_to_the_complete_conservative_set() {
        let handles = handles(2);
        let mut frame = JvmFrameRecord::new(2, 0);
        frame
            .locals_mut()
            .store(0, JvmValue::Reference(JvmReference::managed(handles[0])))
            .unwrap();
        assert_eq!(frame.safepoint_roots(), &[handles[0].id()]);

        // Simulate a stale bitmap while retaining the uncertainty signal set by
        // mutable access. Uncertainty, rather than the bitmap alone, controls trust.
        frame
            .locals_mut()
            .store(1, JvmValue::Reference(JvmReference::managed(handles[1])))
            .unwrap();
        frame.dirty_locals.fill(false);

        assert_eq!(frame.safepoint_roots(), &[handles[0].id(), handles[1].id()]);
    }

    #[test]
    #[should_panic(expected = "prepared JVM root map diverged")]
    fn test_safepoints_hard_fail_if_a_trusted_map_diverges() {
        let handles = handles(1);
        let mut frame = JvmFrameRecord::new(1, 0);
        frame
            .locals_mut()
            .store(0, JvmValue::Reference(JvmReference::managed(handles[0])))
            .unwrap();
        let _ = frame.safepoint_roots();
        frame.prepared_roots.clear();
        let _ = frame.safepoint_roots();
    }
}
