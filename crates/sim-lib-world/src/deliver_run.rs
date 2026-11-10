// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One durable delivery attempt and its recovery on the same journal.
//!
//! The first outcome admission is cut. Recovery observes that open journal
//! before any further performance. The performer counter lives outside the
//! performer.

#[cfg(test)]
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use sim_kernel::{CapabilityName, Datum, Symbol};
use sim_lib_journal::{Admission, JournalBackend, JournalError, JournalHead, MemoryBackend};
use sim_lib_operation_gate::{
    FencedDispatch, LeaseClock, LeaseClockReading, LeaseWindow, LifecyclePerformer,
    LifecyclePerformerResponse, OperationError, OperationGrant, OperationIntent,
    OperationLifecycle, OperationOutcome, PostconditionObserver, PostconditionRequest,
    PostconditionResponse, ReplayPolicy,
};

use crate::WorldError;

use super::Reading;

const CLOCK_DOMAIN: &str = "world/deliver-ticks";
const HOLDER: &str = "world/deliver-holder";
const START_TICK: u64 = 10;
const TICK_SPAN: u64 = 10;

pub(super) struct Qualified {
    pub(super) acknowledgement: &'static str,
    pub(super) cause: &'static str,
    pub(super) class: &'static str,
    pub(super) desired: String,
    pub(super) dispatches: u64,
    pub(super) intent: String,
    pub(super) observations: u64,
    pub(super) performer_calls: u64,
    pub(super) provenance: String,
    pub(super) selector: String,
    pub(super) state: String,
    pub(super) value: Option<String>,
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

    fn set(&self, tick: u64) {
        self.tick.store(tick, Ordering::SeqCst);
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

/// Cuts the first outcome admission so recovery must observe the open journal.
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
            return Err(JournalError::InjectedCrash("before delivery outcome"));
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

#[derive(Clone, Copy)]
enum Condition {
    Satisfied,
    Divergent,
    Partial,
    Unavailable,
}

struct Sink {
    calls: Arc<AtomicUsize>,
    clock: Arc<TickClock>,
    advance_to: u64,
    condition: Condition,
}

impl LifecyclePerformer for Sink {
    fn identity(&self) -> Datum {
        Datum::String("performer/world-deliver".into())
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.condition, Condition::Divergent | Condition::Partial) {
            self.clock.set(self.advance_to);
        }
        LifecyclePerformerResponse::AcknowledgementMissing
    }
}

struct Watch {
    calls: Arc<AtomicUsize>,
    condition: Condition,
    observed: Datum,
    cause: &'static str,
}

impl PostconditionObserver for Watch {
    fn identity(&self) -> Datum {
        Datum::String("observer/world-deliver".into())
    }

    fn observe(&mut self, request: &PostconditionRequest) -> PostconditionResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let evidence = Datum::String(self.cause.to_owned());
        match self.condition {
            Condition::Satisfied => PostconditionResponse::Satisfied {
                observed: request.expected().clone(),
                evidence,
            },
            Condition::Divergent => PostconditionResponse::NotSatisfied {
                observed: self.observed.clone(),
                evidence,
            },
            Condition::Partial if request.dispatch().is_none() => {
                PostconditionResponse::NotSatisfied {
                    observed: self.observed.clone(),
                    evidence,
                }
            }
            Condition::Partial | Condition::Unavailable => {
                PostconditionResponse::Unavailable { reason: evidence }
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static QUALIFY_CALLS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn qualify_calls() -> usize {
    QUALIFY_CALLS.with(Cell::get)
}

pub(super) fn qualify(
    realization: &str,
    selector: &str,
    desired: &str,
    reason: &str,
    reading: &Reading,
) -> Result<Option<Qualified>, WorldError> {
    #[cfg(test)]
    QUALIFY_CALLS.with(|cell| cell.set(cell.get().saturating_add(1)));
    if super::private_input(
        realization,
        selector,
        desired,
        reason,
        reading.value.as_deref(),
    ) {
        return Ok(None);
    }
    let cause = cause(reason, &reading.state, &reading.kind)?;
    let condition = condition(cause);
    let calls = Arc::new(AtomicUsize::new(0));
    let observations = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(TickClock::new(START_TICK));
    let backend = Arc::new(OutcomeCut::new());
    let intent = OperationIntent::new(
        format!("world/deliver/{selector}"),
        Datum::String(selector.to_owned()),
        Datum::String(desired.to_owned()),
        ReplayPolicy::ExactlyOnce,
    )
    .map_err(operation_error)?;
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("world/deliver"),
        Datum::String(format!("realization/{realization}")),
    )
    .map_err(operation_error)?;
    let mut sink = Sink {
        calls: Arc::clone(&calls),
        clock: Arc::clone(&clock),
        advance_to: START_TICK + TICK_SPAN,
        condition,
    };
    let mut watch = Watch {
        calls: Arc::clone(&observations),
        condition,
        observed: Datum::String(reading.value.clone().unwrap_or_default()),
        cause,
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
                "delivery persisted an outcome before recovery".to_owned(),
            ));
        }
    }
    let outcome = lifecycle(&backend, &clock)
        .run(&intent, &grant, window(clock.get())?, &mut sink, &mut watch)
        .map_err(operation_error)?;
    let record = OperationLifecycle::from_shared(Arc::clone(&backend))
        .record(intent.id())
        .map_err(operation_error)?
        .ok_or_else(|| WorldError::Qualification("delivery record is missing".to_owned()))?;
    let dispatches = u64::try_from(record.dispatches().len())
        .map_err(|_| WorldError::Qualification("delivery dispatch count overflowed".to_owned()))?;
    let performer_calls = count(&calls)?;
    let observation_count = count(&observations)?;
    if performer_calls != dispatches {
        return Err(WorldError::Qualification(
            "performer counter disagrees with durable dispatches".to_owned(),
        ));
    }
    let class = class_of(&outcome);
    if class != required_class(cause) || dispatches != required_dispatches(cause) {
        return Err(WorldError::Qualification(format!(
            "delivery outcome {class} with {dispatches} dispatches does not match {cause}"
        )));
    }
    let minimum = if dispatches == 0 { 2 } else { 3 };
    if observation_count < minimum {
        return Err(WorldError::Qualification(
            "recovery did not observe before completion".to_owned(),
        ));
    }
    Ok(Some(Qualified {
        acknowledgement: acknowledgement(cause),
        cause,
        class,
        desired: desired.to_owned(),
        dispatches,
        intent: required_intent(selector),
        observations: observation_count,
        performer_calls,
        provenance: reading.provenance.clone(),
        selector: selector.to_owned(),
        state: reading.state.clone(),
        value: reading.value.clone(),
    }))
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

fn condition(cause: &'static str) -> Condition {
    match cause {
        "already-true" => Condition::Satisfied,
        "divergent" | "wrong-tag" => Condition::Divergent,
        "partial-success" => Condition::Partial,
        _ => Condition::Unavailable,
    }
}

fn cause(reason: &str, state: &str, kind: &str) -> Result<&'static str, WorldError> {
    match (reason, state, kind) {
        ("completed", "current", _) => Ok("already-true"),
        ("desired-differs", "current", "tag") => Ok("wrong-tag"),
        ("desired-differs", "current", _) => Ok("divergent"),
        ("original-preserved", "lagging", _) => Ok("registry-lag"),
        ("original-preserved", "stale", _) => Ok("stale-observation"),
        ("original-preserved", "denied", _) => Ok("denied-access"),
        ("original-preserved", "partial", _) => Ok("partial-success"),
        ("original-preserved", "uncertain", _) => Ok("uncertain-reading"),
        ("original-preserved", "unavailable" | "absent", _) => Ok("unavailable-receiver"),
        _ => Err(WorldError::Qualification(format!(
            "delivery has no cause for {reason} in state {state}"
        ))),
    }
}

fn required_class(cause: &str) -> &'static str {
    match cause {
        "already-true" => "already-true",
        "divergent" | "wrong-tag" => "diverged",
        _ => "uncertain",
    }
}

fn required_dispatches(cause: &str) -> u64 {
    match cause {
        "divergent" | "wrong-tag" | "partial-success" => 1,
        _ => 0,
    }
}

fn acknowledgement(cause: &str) -> &'static str {
    match cause {
        "already-true" => "omitted",
        "divergent" | "wrong-tag" | "partial-success" => "missing",
        _ => "unsent",
    }
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
        .map_err(|_| WorldError::Qualification("delivery counter overflowed".to_owned()))
}

fn required_intent(selector: &str) -> String {
    format!("intent/reconcile/{selector}")
}

fn operation_error(error: OperationError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::{Reading, gap, private_input};
    use super::qualify;
    use crate::command::render;

    fn reading(state: &str, value: Option<&str>) -> Reading {
        Reading {
            kind: "branch".to_owned(),
            provenance: "model/git/branch/main".to_owned(),
            state: state.to_owned(),
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn private_inputs_redact_before_the_lifecycle() {
        let secret = "private-material:s3cret-token";
        assert!(private_input(
            "model",
            "git/branch/main",
            secret,
            "completed",
            Some(secret),
        ));
        assert!(!private_input(
            "model",
            "mirror/sim-private",
            "model-commit-main",
            "original-preserved",
            Some("model-commit-main"),
        ));
        let hidden = qualify(
            "model",
            "git/branch/main",
            secret,
            "completed",
            &reading("current", Some(secret)),
        )
        .expect("redacted value");
        assert!(hidden.is_none());
        let hidden_reason = qualify(
            "model",
            "git/branch/main",
            "model-commit-main",
            secret,
            &reading("current", Some("model-commit-main")),
        )
        .expect("redacted reason");
        assert!(hidden_reason.is_none());
        let rendered = render(&gap("model", secret, "private-leakage"));
        assert!(!rendered.contains("s3cret-token"));
        assert!(!rendered.contains("private-material:"));
        let kept = qualify(
            "model",
            "mirror/sim-private",
            "model-commit-main",
            "original-preserved",
            &reading("denied", None),
        )
        .expect("mirror")
        .expect("qualified");
        assert_eq!(kept.cause, "denied-access");
        assert_eq!(kept.selector, "mirror/sim-private");
        assert_eq!(kept.dispatches, 0);
        assert!(kept.value.is_none());
    }
}
