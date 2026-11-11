// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One pass of the operation lifecycle behind the loaded operate command.
//!
//! A verified dispatch is replayed on a new lease without a second call.
//! A missing acknowledgement stays dispatched and is not performed again.
//! A forged transition is refused. Each interruption before and after an
//! admission recovers without repeating the dispatch. The performer
//! counter matches the durable dispatch count.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use sim_kernel::{CapabilityName, Datum, Symbol};
use sim_lib_journal::{Journal, JournalEntry, JournalError, JournalObject, MemoryBackend};
use sim_lib_operation_gate::{
    DurableOperationState, FencedDispatch, LeaseWindow, LifecyclePerformer,
    LifecyclePerformerResponse, OperationAttempt, OperationDispatch, OperationError,
    OperationGrant, OperationIntent, OperationLifecycle, OperationOutcome, OperationPerformer,
    OperationService, PerformerResponse, PostconditionObserver, PostconditionRequest,
    PostconditionResponse, ReplayPolicy, project_verified_lifecycle_records,
};

use crate::WorldError;

use super::cut::CutBackend;

pub(super) struct Measured {
    pub verified: &'static str,
    pub replay: &'static str,
    pub uncertain: &'static str,
    pub forged: &'static str,
    pub interrupts: u64,
    pub recovered: u64,
    pub counter: &'static str,
    pub invokes: u64,
}

pub(super) fn machine() -> Result<Measured, WorldError> {
    let mut invokes = 0_u64;
    let (verified, replay, counter) = verified_replay(&mut invokes)?;
    let uncertain = missing_ack(&mut invokes)?;
    let forged = forged_transition()?;
    let (interrupts, recovered) = interruptions(&mut invokes)?;
    Ok(Measured {
        verified,
        replay,
        uncertain,
        forged,
        interrupts,
        recovered,
        counter,
        invokes,
    })
}

fn verified_replay(
    invokes: &mut u64,
) -> Result<(&'static str, &'static str, &'static str), WorldError> {
    let backend = Arc::new(CutBackend::new());
    let (intent, grant) = intent_grant()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let effect = Arc::new(AtomicBool::new(false));
    let mut performer = Actor {
        calls: Arc::clone(&calls),
        effect: Arc::clone(&effect),
        acknowledge: true,
    };
    let mut observer = Watching {
        effect: Arc::clone(&effect),
    };
    *invokes = invokes.saturating_add(1);
    let first = OperationLifecycle::from_shared(Arc::clone(&backend))
        .run(
            &intent,
            &grant,
            window("holder/a", 10, 20)?,
            &mut performer,
            &mut observer,
        )
        .map_err(|_| broken("operation lifecycle refused the call"))?;
    if !matches!(first, OperationOutcome::Verified { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("operation verification did not dispatch once"));
    }
    let mut reopened = OperationLifecycle::from_shared(Arc::clone(&backend));
    *invokes = invokes.saturating_add(1);
    let second = reopened
        .run(
            &intent,
            &grant,
            window("holder/b", 30, 40)?,
            &mut performer,
            &mut observer,
        )
        .map_err(|_| broken("operation replay failed"))?;
    if !matches!(second, OperationOutcome::Verified { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("operation replay repeated a dispatch"));
    }
    let record = OperationLifecycle::from_shared(backend)
        .record(intent.id())
        .map_err(|_| broken("operation record is unreadable"))?
        .ok_or_else(|| broken("operation record is absent"))?;
    if record.dispatches().len() != calls.load(Ordering::SeqCst) {
        return Err(broken("operation counter disagreed with the record"));
    }
    Ok(("verified", "held", "same"))
}

fn missing_ack(invokes: &mut u64) -> Result<&'static str, WorldError> {
    let backend = Arc::new(MemoryBackend::new());
    let (intent, grant) = intent_grant()?;
    let attempt = OperationAttempt::new(intent.id().clone(), 0)
        .map_err(|_| broken("operation attempt was not canonical"))?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut performer = ServiceActor {
        calls: Arc::clone(&calls),
        acknowledge: false,
    };
    let mut service = OperationService::from_shared(Arc::clone(&backend));
    *invokes = invokes.saturating_add(1);
    service
        .resume()
        .map_err(|_| broken("operation service did not resume"))?;
    let first = service
        .execute(&intent, &grant, &attempt, &mut performer)
        .map_err(|_| broken("missing acknowledgement was not recorded"))?;
    if !matches!(first, DurableOperationState::Dispatched { .. })
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("missing acknowledgement was not uncertain"));
    }
    let replay_grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("world/operate"),
        Datum::String("authority/replay".into()),
    )
    .map_err(|_| broken("operation replay grant was not canonical"))?;
    let replay_attempt = OperationAttempt::new(intent.id().clone(), 1)
        .map_err(|_| broken("operation replay attempt was not canonical"))?;
    let mut replay = ServiceActor {
        calls: Arc::clone(&calls),
        acknowledge: true,
    };
    let mut reopened = OperationService::from_shared(backend);
    *invokes = invokes.saturating_add(1);
    reopened
        .resume()
        .map_err(|_| broken("operation service did not resume"))?;
    let second = reopened
        .execute(&intent, &replay_grant, &replay_attempt, &mut replay)
        .map_err(|_| broken("missing acknowledgement could not be reread"))?;
    if !matches!(second, DurableOperationState::Dispatched { .. })
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("missing acknowledgement was dispatched again"));
    }
    let record = reopened
        .record(intent.id())
        .map_err(|_| broken("operation record is unreadable"))?
        .ok_or_else(|| broken("operation record is absent"))?;
    if record.receipt().is_some() {
        return Err(broken("missing acknowledgement stored a receipt"));
    }
    Ok("uncertain")
}

fn forged_transition() -> Result<&'static str, WorldError> {
    let journal = Journal::new(MemoryBackend::new());
    let lease = journal
        .acquire_lease()
        .map_err(|_| broken("forged operation journal refused a lease"))?;
    let object = JournalObject::from_datum(Datum::String("foreign".into()))
        .map_err(|_| broken("forged operation object was not canonical"))?;
    let entry = JournalEntry::new(
        0,
        None,
        Symbol::qualified("operation", "foreign-transition"),
        vec![object.id.clone()],
    );
    journal
        .publish(&lease, None, vec![object], vec![entry])
        .map_err(|_| broken("forged operation journal refused the write"))?;
    let snapshot = journal
        .verified_snapshot()
        .map_err(|_| broken("forged operation snapshot was not verified"))?;
    if project_verified_lifecycle_records(&snapshot).is_ok() {
        return Err(broken("forged operation record was admitted"));
    }
    Ok("refused")
}

fn interruptions(invokes: &mut u64) -> Result<(u64, u64), WorldError> {
    let mut interrupts = 0_u64;
    let mut recovered = 0_u64;
    for admission in 1..=7 {
        for after_commit in [false, true] {
            let backend = Arc::new(CutBackend::new());
            backend.cut(admission, after_commit);
            let (intent, grant) = intent_grant()?;
            let calls = Arc::new(AtomicUsize::new(0));
            let effect = Arc::new(AtomicBool::new(false));
            let mut performer = Actor {
                calls: Arc::clone(&calls),
                effect: Arc::clone(&effect),
                acknowledge: true,
            };
            let mut observer = Watching {
                effect: Arc::clone(&effect),
            };
            let crashed = OperationLifecycle::from_shared(Arc::clone(&backend)).run(
                &intent,
                &grant,
                window("holder/a", 10, 20)?,
                &mut performer,
                &mut observer,
            );
            *invokes = invokes.saturating_add(1);
            if !matches!(
                crashed,
                Err(OperationError::Journal(JournalError::InjectedCrash(_)))
            ) {
                return Err(broken("operation interruption missed a boundary"));
            }
            interrupts = interrupts.saturating_add(1);
            backend.clear();
            let restored = OperationLifecycle::from_shared(backend).run(
                &intent,
                &grant,
                window("holder/b", 30, 40)?,
                &mut performer,
                &mut observer,
            );
            *invokes = invokes.saturating_add(1);
            let calls_after = calls.load(Ordering::SeqCst);
            if !matches!(
                restored,
                Ok(OperationOutcome::Verified { .. } | OperationOutcome::Diverged { .. })
            ) || calls_after > 1
            {
                return Err(broken("operation interruption repeated a dispatch"));
            }
            recovered = recovered.saturating_add(1);
        }
    }
    Ok((interrupts, recovered))
}

fn intent_grant() -> Result<(OperationIntent, OperationGrant), WorldError> {
    let intent = OperationIntent::new(
        "world/operate",
        Datum::String("target/operate".into()),
        Datum::String("present".into()),
        ReplayPolicy::ExactlyOnce,
    )
    .map_err(|_| broken("operation intent was not canonical"))?;
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("world/operate"),
        Datum::String("authority/operate".into()),
    )
    .map_err(|_| broken("operation grant was not canonical"))?;
    Ok((intent, grant))
}

fn window(holder: &str, acquired: u64, expires: u64) -> Result<LeaseWindow, WorldError> {
    LeaseWindow::new(Datum::String(holder.into()), acquired, expires)
        .map_err(|_| broken("operation lease was not canonical"))
}

fn broken(reason: &str) -> WorldError {
    WorldError::Qualification(reason.to_owned())
}

struct Actor {
    calls: Arc<AtomicUsize>,
    effect: Arc<AtomicBool>,
    acknowledge: bool,
}

impl LifecyclePerformer for Actor {
    fn identity(&self) -> Datum {
        Datum::String("performer/operate".into())
    }

    fn perform(&mut self, dispatch: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.effect.store(true, Ordering::SeqCst);
        if self.acknowledge {
            LifecyclePerformerResponse::Receipt(Datum::String(dispatch.id().to_string()))
        } else {
            LifecyclePerformerResponse::AcknowledgementMissing
        }
    }
}

struct Watching {
    effect: Arc<AtomicBool>,
}

impl PostconditionObserver for Watching {
    fn identity(&self) -> Datum {
        Datum::String("observer/operate".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        if self.effect.load(Ordering::SeqCst) {
            PostconditionResponse::Satisfied {
                observed: Datum::String("present".into()),
                evidence: Datum::String("observer/present".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::String("absent".into()),
                evidence: Datum::String("observer/absent".into()),
            }
        }
    }
}

struct ServiceActor {
    calls: Arc<AtomicUsize>,
    acknowledge: bool,
}

impl OperationPerformer for ServiceActor {
    fn perform(&mut self, _: &OperationDispatch) -> PerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.acknowledge {
            PerformerResponse::Receipt(Datum::String("raw-ok".into()))
        } else {
            PerformerResponse::AcknowledgementMissing
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_loaded_machine_measures_replay_uncertainty_and_interruption() {
        let measured = super::machine().expect("operation machine");
        assert_eq!(measured.verified, "verified");
        assert_eq!(measured.replay, "held");
        assert_eq!(measured.uncertain, "uncertain");
        assert_eq!(measured.forged, "refused");
        assert_eq!(measured.interrupts, 14);
        assert_eq!(measured.recovered, 14);
        assert_eq!(measured.counter, "same");
        assert_eq!(measured.invokes, 32);
    }
}
