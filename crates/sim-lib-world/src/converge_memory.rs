// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained converge records.
//!
//! One named case is driven once. The caller that finds the case absent
//! reserves it before the revision runs. A concurrent or later call with the
//! same case waits and returns that record. A dropped reservation publishes
//! nothing. This table does not store a proof leaf.

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex};

use sim_kernel::Datum;

enum Slot {
    Pending { waiters: usize },
    Ready(Box<Datum>),
}

struct Table {
    slots: HashMap<String, Slot>,
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
    Ready(Datum),
    Mine(Claim),
}

pub(super) struct Claim {
    case: String,
    open: bool,
}

pub(super) fn claim(case: &str) -> Reservation {
    let mut guard = lock();
    loop {
        match guard.slots.get(case) {
            Some(Slot::Ready(found)) => return Reservation::Ready(found.as_ref().clone()),
            Some(Slot::Pending { .. }) => {
                if let Some(Slot::Pending { waiters }) = guard.slots.get_mut(case) {
                    *waiters += 1;
                }
                guard = cond()
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(Slot::Pending { waiters }) = guard.slots.get_mut(case) {
                    *waiters = waiters.saturating_sub(1);
                }
            }
            None => {
                guard
                    .slots
                    .insert(case.to_owned(), Slot::Pending { waiters: 0 });
                drop(guard);
                #[cfg(test)]
                note_and_wait(case);
                return Reservation::Mine(Claim {
                    case: case.to_owned(),
                    open: true,
                });
            }
        }
    }
}

impl Claim {
    pub(super) fn commit(mut self, record: &Datum) {
        {
            let mut guard = lock();
            guard
                .slots
                .insert(self.case.clone(), Slot::Ready(Box::new(record.clone())));
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
            if matches!(guard.slots.get(&self.case), Some(Slot::Pending { .. })) {
                guard.slots.remove(&self.case);
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
    case: String,
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
fn note_and_wait(case: &str) {
    {
        let mut opened = OPENED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *opened.entry(case.to_owned()).or_default() += 1;
    }
    let mut guard = hold_lock();
    if guard
        .as_ref()
        .is_some_and(|hold| hold.case == case && !hold.release)
    {
        if let Some(hold) = guard.as_mut() {
            hold.owners += 1;
        }
        while guard
            .as_ref()
            .is_some_and(|hold| hold.case == case && !hold.release)
        {
            guard = HOLD
                .1
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[cfg(test)]
fn arm_hold(case: &str) {
    *hold_lock() = Some(Hold {
        case: case.to_owned(),
        owners: 0,
        release: false,
    });
}

#[cfg(test)]
fn release_hold(case: &str) {
    {
        let mut guard = hold_lock();
        if let Some(hold) = guard.as_mut()
            && hold.case == case
        {
            hold.release = true;
        }
    }
    HOLD.1.notify_all();
}

#[cfg(test)]
fn held_owners(case: &str) -> usize {
    hold_lock()
        .as_ref()
        .filter(|hold| hold.case == case)
        .map(|hold| hold.owners)
        .unwrap_or(0)
}

#[cfg(test)]
fn opened(case: &str) -> usize {
    OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(case)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
fn pending_waiters(case: &str) -> usize {
    match lock().slots.get(case) {
        Some(Slot::Pending { waiters }) => *waiters,
        _ => 0,
    }
}

#[cfg(test)]
fn hold_lock() -> std::sync::MutexGuard<'static, Option<Hold>> {
    HOLD.0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, Instant};

    use sim_kernel::Datum;

    use super::{Reservation, arm_hold, claim, held_owners, opened, pending_waiters, release_hold};
    use crate::command::render;

    const PARALLEL: &str = "same 1 failures a:proof-failed,b:proof-failed";

    #[test]
    fn a_repeated_case_is_driven_once() {
        let first = super::super::run("parallel").expect("parallel");
        let seen = opened("parallel");
        let second = super::super::run("parallel").expect("repeat");
        assert_eq!(first, second);
        assert_eq!(opened("parallel"), seen);
        assert_eq!(seen, 1);
        assert_eq!(text(&first, "case"), "parallel");
        assert_eq!(text(&first, "detail"), PARALLEL);
        assert_eq!(count(&first, "invokes"), "2");
        assert_eq!(count(&first, "effects"), "0");
        assert_eq!(count(&first, "subprocesses"), "0");
    }

    #[test]
    fn a_repeated_preserve_is_driven_once() {
        let first = super::super::run("preserve").expect("preserve");
        let seen = opened("preserve");
        let second = super::super::run("preserve").expect("repeat");
        assert_eq!(first, second);
        assert_eq!(opened("preserve"), seen);
        assert_eq!(seen, 1);
        assert!(text(&first, "detail").starts_with("self-approval held 1 "));
        assert!(text(&first, "detail").contains(" later uncertain"));
        assert_eq!(count(&first, "invokes"), "5");
        assert_eq!(count(&first, "effects"), "0");
    }

    #[test]
    fn a_private_case_is_refused_before_reservation() {
        let secret = "private-material:s3cret-token";
        let value = super::super::run(secret).expect("private");
        assert_eq!(text(&value, "reason"), "private-leakage");
        assert_eq!(text(&value, "case"), "");
        assert_eq!(opened(secret), 0);
        let rendered = render(&value);
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("s3cret-token"));
    }

    #[test]
    fn an_unknown_case_is_refused_before_reservation() {
        let value = super::super::run("unknown-case").expect("unknown");
        assert_eq!(text(&value, "reason"), "unknown-case");
        assert_eq!(text(&value, "case"), "unknown-case");
        assert_eq!(opened("unknown-case"), 0);
    }

    #[test]
    fn a_dropped_reservation_wakes_the_next_caller() {
        let case = "converge-memory-drop";
        let Reservation::Mine(held) = claim(case) else {
            panic!("the first caller reserves the case");
        };
        let waiter = thread::spawn(|| claim(case));
        let deadline = Instant::now() + Duration::from_secs(5);
        while pending_waiters(case) == 0 {
            assert!(Instant::now() < deadline, "the waiter did not arrive");
            thread::yield_now();
        }
        drop(held);
        let Reservation::Mine(next) = waiter.join().expect("waiter") else {
            panic!("a dropped reservation does not publish a record");
        };
        drop(next);
    }

    #[test]
    fn concurrent_identical_cases_share_one_receipt() {
        let case = "converge-memory-race";
        arm_hold(case);
        let start = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let start = Arc::clone(&start);
            threads.push(thread::spawn(move || {
                start.wait();
                share(case)
            }));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let owners = held_owners(case);
            let waiters = pending_waiters(case);
            if owners >= 2 || (owners == 1 && waiters >= 1) {
                break;
            }
            if Instant::now() >= deadline {
                release_hold(case);
                for thread in threads {
                    let _ = thread.join();
                }
                panic!("owners {owners} waiters {waiters} opened {}", opened(case));
            }
            thread::yield_now();
        }
        release_hold(case);
        let mut results = Vec::new();
        for thread in threads {
            results.push(thread.join().expect("join"));
        }
        assert_eq!(results[0], results[1]);
        assert_eq!(opened(case), 1);
        assert_eq!(text(&results[0], "case"), "parallel");
        assert_eq!(text(&results[0], "detail"), PARALLEL);
        assert_eq!(count(&results[0], "invokes"), "2");
    }

    fn share(case: &str) -> Datum {
        super::super::run(case).expect("raced converge")
    }

    fn text(value: &Datum, name: &str) -> String {
        match field(value, name) {
            Some(Datum::String(text)) => text.clone(),
            _ => panic!("missing {name}"),
        }
    }

    fn count(value: &Datum, name: &str) -> String {
        match field(value, name) {
            Some(Datum::Number(number)) => number.canonical.clone(),
            _ => panic!("missing {name}"),
        }
    }

    fn field<'a>(value: &'a Datum, name: &str) -> Option<&'a Datum> {
        let Datum::Node { fields, .. } = value else {
            return None;
        };
        fields
            .iter()
            .find(|(field, _)| field.to_string() == name)
            .map(|(_, field)| field)
    }
}
