// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Remembered model canary deliveries.
//!
//! One reading opens one reconciliation. The caller that finds the reading
//! absent reserves it before that reconciliation. A concurrent or later call
//! with the same reading waits and returns the delivery instead of opening
//! another journal. A different reading reconciles again. A dropped
//! reservation publishes nothing.

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex};

use sim_kernel::Datum;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    realization: String,
    source: String,
    destination: String,
    purpose: String,
    kind: String,
    provenance: String,
    state: String,
    value: String,
}

enum Slot {
    Pending { waiters: usize },
    Ready(Box<Datum>),
}

struct Table {
    slots: HashMap<Key, Slot>,
}

static TABLE: LazyLock<(Mutex<Table>, Condvar)> = LazyLock::new(|| {
    (
        Mutex::new(Table {
            slots: HashMap::new(),
        }),
        Condvar::new(),
    )
});

pub(super) struct Input<'a> {
    pub(super) realization: &'a str,
    pub(super) source: &'a str,
    pub(super) destination: &'a str,
    pub(super) purpose: &'a str,
    pub(super) kind: &'a str,
    pub(super) provenance: &'a str,
    pub(super) state: &'a str,
    pub(super) value: &'a str,
}

pub(super) enum Reservation {
    Ready(Datum),
    Mine(Claim),
}

pub(super) struct Claim {
    key: Key,
    open: bool,
}

pub(super) fn claim(input: Input<'_>) -> Reservation {
    let key = Key {
        realization: input.realization.to_owned(),
        source: input.source.to_owned(),
        destination: input.destination.to_owned(),
        purpose: input.purpose.to_owned(),
        kind: input.kind.to_owned(),
        provenance: input.provenance.to_owned(),
        state: input.state.to_owned(),
        value: input.value.to_owned(),
    };
    let mut guard = lock();
    loop {
        match guard.slots.get(&key) {
            Some(Slot::Ready(found)) => return Reservation::Ready(found.as_ref().clone()),
            Some(Slot::Pending { .. }) => {
                if let Some(Slot::Pending { waiters }) = guard.slots.get_mut(&key) {
                    *waiters += 1;
                }
                guard = cond()
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(Slot::Pending { waiters }) = guard.slots.get_mut(&key) {
                    *waiters = waiters.saturating_sub(1);
                }
            }
            None => {
                guard
                    .slots
                    .insert(key.clone(), Slot::Pending { waiters: 0 });
                drop(guard);
                #[cfg(test)]
                note_and_wait(&key.destination);
                return Reservation::Mine(Claim { key, open: true });
            }
        }
    }
}

impl Claim {
    pub(super) fn commit(mut self, delivered: &Datum) {
        {
            let mut guard = lock();
            guard
                .slots
                .insert(self.key.clone(), Slot::Ready(Box::new(delivered.clone())));
        }
        self.open = false;
        cond().notify_all();
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if !self.open {
            return;
        }
        {
            let mut guard = lock();
            if matches!(guard.slots.get(&self.key), Some(Slot::Pending { .. })) {
                guard.slots.remove(&self.key);
            }
        }
        cond().notify_all();
    }
}

fn lock() -> std::sync::MutexGuard<'static, Table> {
    TABLE
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn cond() -> &'static Condvar {
    &TABLE.1
}

#[cfg(test)]
struct Hold {
    destination: String,
    owners: usize,
    release: bool,
}

#[cfg(test)]
static HOLD: LazyLock<(Mutex<Option<Hold>>, Condvar)> =
    LazyLock::new(|| (Mutex::new(None), Condvar::new()));

#[cfg(test)]
static OPENED: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
pub(super) fn note_and_wait(destination: &str) {
    {
        let mut opened = OPENED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *opened.entry(destination.to_owned()).or_default() += 1;
    }
    let mut guard = hold_lock();
    if guard
        .as_ref()
        .is_some_and(|hold| hold.destination == destination && !hold.release)
    {
        if let Some(hold) = guard.as_mut() {
            hold.owners += 1;
        }
        while guard
            .as_ref()
            .is_some_and(|hold| hold.destination == destination && !hold.release)
        {
            guard = HOLD
                .1
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[cfg(test)]
pub(crate) fn arm_hold(destination: &str) {
    *hold_lock() = Some(Hold {
        destination: destination.to_owned(),
        owners: 0,
        release: false,
    });
}

#[cfg(test)]
pub(crate) fn release_hold(destination: &str) {
    {
        let mut guard = hold_lock();
        if let Some(hold) = guard.as_mut()
            && hold.destination == destination
        {
            hold.release = true;
        }
    }
    HOLD.1.notify_all();
}

#[cfg(test)]
pub(crate) fn held_owners(destination: &str) -> usize {
    hold_lock()
        .as_ref()
        .filter(|hold| hold.destination == destination)
        .map(|hold| hold.owners)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn opened(destination: &str) -> usize {
    OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(destination)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn pending_waiters(destination: &str) -> usize {
    lock()
        .slots
        .iter()
        .filter(|(key, _)| key.destination == destination)
        .map(|(_, slot)| match slot {
            Slot::Pending { waiters } => *waiters,
            Slot::Ready(_) => 0,
        })
        .sum()
}

#[cfg(test)]
fn hold_lock() -> std::sync::MutexGuard<'static, Option<Hold>> {
    HOLD.0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Reservation, claim, pending_waiters};

    #[test]
    fn a_dropped_reservation_wakes_the_next_caller() {
        let destination = "git/branch/canary-memory-drop";
        let Reservation::Mine(held) = claim(super::Input {
            realization: "model",
            source: "git/branch/main",
            destination,
            purpose: "world/model-canary",
            kind: "branch",
            provenance: "model/git/branch/main",
            state: "current",
            value: "model-commit-main",
        }) else {
            panic!("the first caller reserves the reading");
        };
        let waiter = thread::spawn(|| {
            claim(super::Input {
                realization: "model",
                source: "git/branch/main",
                destination,
                purpose: "world/model-canary",
                kind: "branch",
                provenance: "model/git/branch/main",
                state: "current",
                value: "model-commit-main",
            })
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while pending_waiters(destination) == 0 {
            assert!(Instant::now() < deadline, "the waiter did not arrive");
            thread::yield_now();
        }
        drop(held);
        let Reservation::Mine(next) = waiter.join().expect("waiter") else {
            panic!("a dropped reservation does not publish a delivery");
        };
        drop(next);
    }
}
