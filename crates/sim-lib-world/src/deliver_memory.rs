// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Remembered delivery qualifications.
//!
//! One reading opens one journal. The caller that finds the reading absent
//! reserves it before opening that journal. A concurrent or later call with
//! the same reading waits for that reservation and returns the qualification
//! instead of opening another journal. A different reading qualifies again.

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex};

use super::super::Reading;
use super::Qualified;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    realization: String,
    selector: String,
    desired: String,
    reason: String,
    kind: String,
    state: String,
    value: Option<String>,
    provenance: String,
}

enum Slot {
    Pending { waiters: usize },
    Ready(Box<Qualified>),
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

pub(super) enum Reservation {
    Ready(Qualified),
    Mine(Claim),
}

pub(super) struct Claim {
    key: Key,
    open: bool,
}

pub(super) fn claim(
    realization: &str,
    selector: &str,
    desired: &str,
    reason: &str,
    reading: &Reading,
) -> Reservation {
    let key = key(realization, selector, desired, reason, reading);
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
                return Reservation::Mine(Claim { key, open: true });
            }
        }
    }
}

impl Claim {
    pub(super) fn commit(mut self, qualified: &Qualified) {
        {
            let mut guard = lock();
            guard
                .slots
                .insert(self.key.clone(), Slot::Ready(Box::new(qualified.clone())));
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

fn key(realization: &str, selector: &str, desired: &str, reason: &str, reading: &Reading) -> Key {
    Key {
        realization: realization.to_owned(),
        selector: selector.to_owned(),
        desired: desired.to_owned(),
        reason: reason.to_owned(),
        kind: reading.kind.clone(),
        state: reading.state.clone(),
        value: reading.value.clone(),
        provenance: reading.provenance.clone(),
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
    desired: String,
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
pub(super) fn note_and_wait(desired: &str) {
    {
        let mut opened = OPENED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *opened.entry(desired.to_owned()).or_default() += 1;
    }
    let mut guard = hold_lock();
    if guard
        .as_ref()
        .is_some_and(|hold| hold.desired == desired && !hold.release)
    {
        if let Some(hold) = guard.as_mut() {
            hold.owners += 1;
        }
        while guard
            .as_ref()
            .is_some_and(|hold| hold.desired == desired && !hold.release)
        {
            guard = HOLD
                .1
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[cfg(test)]
pub(crate) fn arm_hold(desired: &str) {
    *hold_lock() = Some(Hold {
        desired: desired.to_owned(),
        owners: 0,
        release: false,
    });
}

#[cfg(test)]
pub(crate) fn release_hold(desired: &str) {
    {
        let mut guard = hold_lock();
        if let Some(hold) = guard.as_mut()
            && hold.desired == desired
        {
            hold.release = true;
        }
    }
    HOLD.1.notify_all();
}

#[cfg(test)]
pub(crate) fn held_owners(desired: &str) -> usize {
    hold_lock()
        .as_ref()
        .filter(|hold| hold.desired == desired)
        .map(|hold| hold.owners)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn opened(desired: &str) -> usize {
    OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(desired)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn pending_waiters(desired: &str) -> usize {
    lock()
        .slots
        .iter()
        .filter(|(key, _)| key.desired == desired)
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

    use super::super::super::Reading;
    use super::{Reservation, claim, pending_waiters};

    fn reading() -> Reading {
        Reading {
            kind: "ci".to_owned(),
            provenance: "model/ci/sim-run".to_owned(),
            state: "partial".to_owned(),
            value: Some("pending".to_owned()),
        }
    }

    #[test]
    fn a_dropped_reservation_wakes_the_next_caller() {
        let desired = "model-commit-memory-drop";
        let Reservation::Mine(held) = claim(
            "model",
            "ci/sim-run",
            desired,
            "original-preserved",
            &reading(),
        ) else {
            panic!("the first caller reserves the reading");
        };
        let waiter = thread::spawn(|| {
            claim(
                "model",
                "ci/sim-run",
                desired,
                "original-preserved",
                &reading(),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while pending_waiters(desired) == 0 {
            assert!(Instant::now() < deadline, "the waiter did not arrive");
            thread::yield_now();
        }
        drop(held);
        let Reservation::Mine(next) = waiter.join().expect("waiter") else {
            panic!("a dropped reservation does not publish a qualification");
        };
        drop(next);
    }
}
