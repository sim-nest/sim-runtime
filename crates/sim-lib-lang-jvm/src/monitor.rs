//! Reentrant monitor state for the deliberately single-lane JVM runtime.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sim_lib_control::CleanupStack;
use sim_lib_mutation::ManagedHandle;

use crate::FailureCondition;

/// Stable identity of the one guest execution lane using a monitor table.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MonitorLane(pub u64);

/// A failed guest monitor operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorError {
    condition: FailureCondition,
}

impl MonitorError {
    /// Guest throwable condition raised by the failed operation.
    pub const fn condition(self) -> FailureCondition {
        self.condition
    }
}

#[derive(Clone, Copy, Debug)]
struct MonitorRecord {
    owner: MonitorLane,
    recursion: usize,
}

#[derive(Clone, Copy, Debug)]
struct Acquisition {
    object: ManagedHandle,
    lane: MonitorLane,
    live: bool,
}

#[derive(Default)]
struct MonitorState {
    records: HashMap<ManagedHandle, MonitorRecord>,
    acquisitions: Vec<Acquisition>,
    releases: Vec<ManagedHandle>,
}

/// Shared single-lane monitor table with structured unwind registration.
///
/// The table uses ordinary guest-machine state, never a host synchronization
/// primitive. Clones refer to the same table so cleanup callbacks can release
/// acquisitions after the owning frame starts to unwind.
#[derive(Clone, Default)]
pub struct MonitorTable(Rc<RefCell<MonitorState>>);

impl MonitorTable {
    /// Creates an empty monitor table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enters `object`, registering this exact acquisition for structured unwind.
    pub fn enter<U: 'static>(
        &self,
        lane: MonitorLane,
        object: ManagedHandle,
        cleanups: &mut CleanupStack<U>,
    ) {
        let token = {
            let mut state = self.0.borrow_mut();
            let record = state.records.entry(object).or_insert(MonitorRecord {
                owner: lane,
                recursion: 0,
            });
            debug_assert_eq!(record.owner, lane, "single-lane table cannot change owner");
            record.recursion += 1;
            let token = state.acquisitions.len();
            state.acquisitions.push(Acquisition {
                object,
                lane,
                live: true,
            });
            token
        };
        let monitors = self.clone();
        cleanups.push(move |_| monitors.release_token(token));
    }

    /// Exits the newest live acquisition of `object` owned by `lane`.
    pub fn exit(&self, lane: MonitorLane, object: ManagedHandle) -> Result<(), MonitorError> {
        let token = {
            let state = self.0.borrow();
            state
                .acquisitions
                .iter()
                .enumerate()
                .rev()
                .find(|(_, acquisition)| {
                    acquisition.live && acquisition.object == object && acquisition.lane == lane
                })
                .map(|(token, _)| token)
        }
        .ok_or(MonitorError {
            condition: FailureCondition::IllegalMonitorState,
        })?;
        self.release_token(token);
        Ok(())
    }

    /// Current recursion count, or zero when the object is not owned.
    pub fn recursion(&self, object: ManagedHandle) -> usize {
        self.0
            .borrow()
            .records
            .get(&object)
            .map_or(0, |record| record.recursion)
    }

    /// Objects released so far, in exact release order.
    pub fn release_order(&self) -> Vec<ManagedHandle> {
        self.0.borrow().releases.clone()
    }

    fn release_token(&self, token: usize) {
        let mut state = self.0.borrow_mut();
        let Some(acquisition) = state.acquisitions.get_mut(token) else {
            return;
        };
        if !acquisition.live {
            return;
        }
        acquisition.live = false;
        let object = acquisition.object;
        let record = state
            .records
            .get_mut(&object)
            .expect("a live acquisition has a monitor record");
        record.recursion -= 1;
        if record.recursion == 0 {
            state.records.remove(&object);
        }
        state.releases.push(object);
    }
}
