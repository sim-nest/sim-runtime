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
}

impl JvmFrameRecord {
    fn new(slots: usize, operands: usize) -> Self {
        Self {
            locals: SlotFile::new(AdmissionLimit(slots)),
            operands: UnitStack::new(WorkLimit(operands)),
            operand_limit: operands,
        }
    }

    fn clear(&mut self) {
        for slot in 0..self.locals.limit() {
            let _ = self.locals.release(slot);
        }
        self.operands.clear();
    }

    /// Returns the frame's bounded local-slot file.
    pub fn locals(&self) -> &SlotFile<JvmValueWidth> {
        &self.locals
    }

    /// Returns the mutable bounded local-slot file.
    pub fn locals_mut(&mut self) -> &mut SlotFile<JvmValueWidth> {
        &mut self.locals
    }

    /// Returns the frame's bounded operand stack.
    pub fn operands(&self) -> &UnitStack<JvmValueWidth> {
        &self.operands
    }

    /// Returns the mutable bounded operand stack.
    pub fn operands_mut(&mut self) -> &mut UnitStack<JvmValueWidth> {
        &mut self.operands
    }

    /// Returns the admitted operand depth for this record.
    pub const fn operand_limit(&self) -> usize {
        self.operand_limit
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
