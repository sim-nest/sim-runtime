// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One model-branch delivery, its recovery, and the zero-work repeat.
//!
//! The first outcome admission is cut. Recovery observes the same journal
//! before any further performance. The repeat reads the persisted outcome
//! and does not perform. The performer counter lives outside the performer.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_kernel::{CapabilityName, Datum, Symbol};
use sim_lib_journal::{Admission, JournalBackend, JournalError, JournalHead, MemoryBackend};
use sim_lib_operation_gate::{
    FencedDispatch, LeaseClock, LeaseClockReading, LeaseWindow, LifecyclePerformer,
    LifecyclePerformerResponse, OperationError, OperationGrant, OperationIntent,
    OperationLifecycle, OperationOutcome, PostconditionObserver, PostconditionRequest,
    PostconditionResponse, ReplayPolicy,
};

use crate::WorldError;

const CLOCK_DOMAIN: &str = "world/canary-ticks";
const HOLDER: &str = "world/canary-holder";
const START_TICK: u64 = 10;
const TICK_SPAN: u64 = 10;

pub(super) struct Reconciliation {
    pub(super) class: &'static str,
    pub(super) dispatches: u64,
    pub(super) observations: u64,
    pub(super) performer_calls: u64,
    pub(super) repeat_dispatches: u64,
    pub(super) repeat_performer_calls: u64,
}

pub(super) fn reconcile(
    realization: &str,
    destination: &str,
    value: &str,
) -> Result<Reconciliation, WorldError> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observations = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(TickClock::new(START_TICK));
    let backend = Arc::new(OutcomeCut::new());
    let store = Arc::new(Mutex::new(None));
    let intent = OperationIntent::new(
        format!("world/canary/{destination}"),
        Datum::String(destination.to_owned()),
        Datum::String(value.to_owned()),
        ReplayPolicy::ExactlyOnce,
    )
    .map_err(operation_error)?;
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("world/model-canary"),
        Datum::String(format!("realization/{realization}")),
    )
    .map_err(operation_error)?;
    let mut sink = Sink {
        calls: Arc::clone(&calls),
        store: Arc::clone(&store),
        value: value.to_owned(),
    };
    let mut watch = Watch {
        calls: Arc::clone(&observations),
        expected: value.to_owned(),
        store: Arc::clone(&store),
    };
    let first = lifecycle(&backend, &clock).run(
        &intent,
        &grant,
        window(clock.get())?,
        &mut sink,
        &mut watch,
    );
    match first {
        Err(OperationError::Journal(JournalError::InjectedCrash(_))) => {}
        Err(error) => return Err(operation_error(error)),
        Ok(_) => {
            return Err(WorldError::Qualification(
                "canary persisted an outcome before recovery".to_owned(),
            ));
        }
    }
    let outcome = lifecycle(&backend, &clock)
        .run(&intent, &grant, window(clock.get())?, &mut sink, &mut watch)
        .map_err(operation_error)?;
    let before = load(&backend, &intent)?;
    let performed = count(&calls)?;
    if performed != before.dispatches {
        return Err(WorldError::Qualification(
            "performer counter disagrees with durable dispatches".to_owned(),
        ));
    }
    if class_of(&outcome) != "verified" || before.dispatches != 1 {
        return Err(WorldError::Qualification(
            "canary recovery did not verify one dispatch".to_owned(),
        ));
    }
    let observed = count(&observations)?;
    if observed < 3 {
        return Err(WorldError::Qualification(
            "recovery did not observe before completion".to_owned(),
        ));
    }
    let stored = store
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if stored.as_deref() != Some(value) {
        return Err(WorldError::Qualification(
            "canary branch does not hold the source head".to_owned(),
        ));
    }
    let repeated = lifecycle(&backend, &clock)
        .run(&intent, &grant, window(clock.get())?, &mut sink, &mut watch)
        .map_err(operation_error)?;
    let after = load(&backend, &intent)?;
    let repeated_calls = count(&calls)?;
    if class_of(&repeated) != "verified" || after.dispatches != before.dispatches {
        return Err(WorldError::Qualification(
            "canary repeat changed the verified delivery".to_owned(),
        ));
    }
    Ok(Reconciliation {
        class: "verified",
        dispatches: before.dispatches,
        observations: observed,
        performer_calls: performed,
        repeat_dispatches: after.dispatches - before.dispatches,
        repeat_performer_calls: repeated_calls - performed,
    })
}

struct Recorded {
    dispatches: u64,
}

fn load(backend: &Arc<OutcomeCut>, intent: &OperationIntent) -> Result<Recorded, WorldError> {
    let record = OperationLifecycle::from_shared(Arc::clone(backend))
        .record(intent.id())
        .map_err(operation_error)?
        .ok_or_else(|| WorldError::Qualification("canary record is missing".to_owned()))?;
    let dispatches = u64::try_from(record.dispatches().len())
        .map_err(|_| WorldError::Qualification("canary dispatch count overflowed".to_owned()))?;
    Ok(Recorded { dispatches })
}

fn class_of(outcome: &OperationOutcome) -> &'static str {
    match outcome {
        OperationOutcome::AlreadyTrue { .. } => "already-true",
        OperationOutcome::Verified { .. } => "verified",
        OperationOutcome::Diverged { .. } => "diverged",
        OperationOutcome::Uncertain { .. } => "uncertain",
    }
}

fn count(counter: &AtomicUsize) -> Result<u64, WorldError> {
    u64::try_from(counter.load(Ordering::SeqCst))
        .map_err(|_| WorldError::Qualification("canary counter overflowed".to_owned()))
}

fn operation_error(error: impl std::fmt::Display) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn lifecycle(backend: &Arc<OutcomeCut>, clock: &Arc<TickClock>) -> OperationLifecycle<OutcomeCut> {
    let clock = Arc::clone(clock);
    let clock: Arc<dyn LeaseClock> = clock;
    OperationLifecycle::from_shared(Arc::clone(backend)).with_clock(clock)
}

fn window(tick: u64) -> Result<LeaseWindow, WorldError> {
    LeaseWindow::new(Datum::String(HOLDER.to_owned()), tick, tick + TICK_SPAN)
        .and_then(|window| window.in_clock(Datum::String(CLOCK_DOMAIN.to_owned())))
        .map_err(operation_error)
}

struct TickClock {
    tick: AtomicU64,
}

impl TickClock {
    fn new(tick: u64) -> Self {
        Self {
            tick: AtomicU64::new(tick),
        }
    }

    fn get(&self) -> u64 {
        self.tick.load(Ordering::SeqCst)
    }
}

impl LeaseClock for TickClock {
    fn read(&self) -> Result<LeaseClockReading, OperationError> {
        Ok(LeaseClockReading {
            domain: Datum::String(CLOCK_DOMAIN.to_owned()),
            tick: self.get(),
        })
    }
}

struct OutcomeCut {
    inner: MemoryBackend,
    armed: AtomicBool,
}

impl OutcomeCut {
    fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            armed: AtomicBool::new(true),
        }
    }
}

impl JournalBackend for OutcomeCut {
    fn acquire_lease(&self) -> Result<sim_lib_journal::Lease, JournalError> {
        self.inner.acquire_lease()
    }

    fn read_state(&self) -> Result<sim_lib_journal::StoredState, JournalError> {
        self.inner.read_state()
    }

    fn admit(&self, admission: Admission) -> Result<JournalHead, JournalError> {
        let outcome = admission.entries().iter().any(|entry| {
            entry.kind == Symbol::qualified("operation", "lifecycle-outcome-persisted")
        });
        if outcome && self.armed.swap(false, Ordering::SeqCst) {
            return Err(JournalError::InjectedCrash("before canary outcome"));
        }
        self.inner.admit(admission)
    }

    fn admit_then(
        &self,
        admission: Admission,
        action: &mut dyn sim_lib_journal::CommitAction,
    ) -> Result<sim_lib_journal::GuardedAdmission, JournalError> {
        self.inner.admit_then(admission, action)
    }

    fn put_datum(
        &self,
        object: sim_lib_journal::JournalObject,
    ) -> Result<sim_lib_journal::StoredDatumRef, JournalError> {
        self.inner.put_datum(object)
    }

    fn get_datum(&self, meaning: &sim_kernel::ContentId) -> Result<Datum, JournalError> {
        self.inner.get_datum(meaning)
    }

    fn rebuild_datum_index(&self) -> Result<Vec<sim_lib_journal::StoredDatumRef>, JournalError> {
        self.inner.rebuild_datum_index()
    }
}

struct Sink {
    calls: Arc<AtomicUsize>,
    store: Arc<Mutex<Option<String>>>,
    value: String,
}

impl LifecyclePerformer for Sink {
    fn identity(&self) -> Datum {
        Datum::String("performer/world-canary".into())
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut slot = self.store.lock().unwrap_or_else(|error| error.into_inner());
        if slot.is_none() {
            *slot = Some(self.value.clone());
        }
        LifecyclePerformerResponse::Receipt(Datum::String(self.value.clone()))
    }
}

struct Watch {
    calls: Arc<AtomicUsize>,
    expected: String,
    store: Arc<Mutex<Option<String>>>,
}

impl PostconditionObserver for Watch {
    fn identity(&self) -> Datum {
        Datum::String("observer/world-canary".into())
    }

    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let slot = self
            .store
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if slot.as_deref() == Some(self.expected.as_str()) {
            PostconditionResponse::Satisfied {
                observed: request.expected().clone(),
                evidence: Datum::String("model-branch-current".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::String(slot.unwrap_or_default()),
                evidence: Datum::String("model-branch-absent".into()),
            }
        }
    }
}
