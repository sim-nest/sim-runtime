// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Operation runs behind the loaded effects command.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use sha2::{Digest, Sha256};
use sim_kernel::{CapabilityName, Datum};
use sim_lib_journal::MemoryBackend;
use sim_lib_operation_gate::{
    DurableOperationState, FencedDispatch, LeaseClock, LeaseClockReading, LeaseWindow,
    LifecyclePerformer, LifecyclePerformerResponse, OperationAttempt, OperationDispatch,
    OperationError, OperationGrant, OperationIntent, OperationLifecycle, OperationOutcome,
    OperationPerformer, OperationService, PerformerResponse, PostconditionObserver,
    PostconditionRequest, PostconditionResponse, ReplayPolicy,
};

use crate::WorldError;

pub(super) struct Class {
    pub(super) detail: String,
    pub(super) digest: String,
    #[cfg(test)]
    pub(super) report: String,
}

pub(super) fn classify(case: &str) -> Result<Class, WorldError> {
    match case {
        "settled" => settled(),
        "dispatched" => dispatched(),
        "divergent" => divergent(),
        "uncertain" => uncertain(),
        "leased" => leased(),
        "replayed" => replayed(),
        "historical" => historical(),
        _ => Err(broken("unknown effects case")),
    }
}

fn settled() -> Result<Class, WorldError> {
    let run = verified("world/effects/settled", 10, 20)?;
    let again = reopen(&run, "holder/settled-again", 20, 30)?;
    if !matches!(again, OperationOutcome::Verified { .. }) || run.calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("settled operation was replayed"));
    }
    let record = lifecycle(&run.backend)
        .record(run.intent.id())
        .map_err(gate)?
        .ok_or_else(|| broken("settled record is absent"))?;
    if record.dispatches().len() != 1
        || !matches!(record.outcome(), Some(OperationOutcome::Verified { .. }))
    {
        return Err(broken("settled record is not one verified dispatch"));
    }
    if record.leases().is_empty()
        || record
            .leases()
            .iter()
            .any(|lease| lease.is_live_at(20) || !lease.expired_as_of(lease.clock(), 20))
    {
        return Err(broken("settled operation kept a live lease"));
    }
    facts(
        "settled",
        [
            "state=verified",
            "dispatched=no",
            "divergent=no",
            "uncertain=no",
            "lease=expired",
            "replay=refused",
            "dispatches=1",
            "calls=1",
        ],
    )
}

fn dispatched() -> Result<Class, WorldError> {
    let run = missing_ack("world/effects/dispatched")?;
    let again = OperationAttempt::new(run.intent.id().clone(), 1).map_err(gate)?;
    let mut performer = MissingPerformer {
        calls: Arc::clone(&run.calls),
    };
    let mut service = OperationService::from_shared(Arc::clone(&run.backend));
    service.resume().map_err(gate)?;
    let state = service
        .execute(&run.intent, &run.grant, &again, &mut performer)
        .map_err(gate)?;
    if !matches!(state, DurableOperationState::Dispatched { .. })
        || run.calls.load(Ordering::SeqCst) != 1
        || service
            .record(run.intent.id())
            .map_err(gate)?
            .ok_or_else(|| broken("dispatched record is absent"))?
            .receipt()
            .is_some()
    {
        return Err(broken(
            "dispatched operation grew a receipt or a second call",
        ));
    }
    facts(
        "dispatched",
        [
            "state=dispatched",
            "receipt=absent",
            "calls=1",
            "second-attempt=refused",
        ],
    )
}

fn divergent() -> Result<Class, WorldError> {
    let domain = Datum::String("world/effects-ticks".into());
    let tick = Arc::new(AtomicU64::new(100));
    let backend = Arc::new(MemoryBackend::new());
    let (intent, grant) = pair("world/effects/divergent")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut performer = ExpiryPerformer {
        calls: Arc::clone(&calls),
        tick: Arc::clone(&tick),
    };
    let mut observer = Absent;
    let clock = Arc::new(SharedClock {
        tick: Arc::clone(&tick),
        domain: domain.clone(),
    });
    let window = LeaseWindow::new(Datum::String("holder/divergent".into()), 100, 120)
        .map_err(gate)?
        .in_clock(domain.clone())
        .map_err(gate)?;
    let first = OperationLifecycle::from_shared(Arc::clone(&backend))
        .with_clock(Arc::clone(&clock) as Arc<dyn LeaseClock>)
        .run(&intent, &grant, window, &mut performer, &mut observer)
        .map_err(gate)?;
    if !matches!(first, OperationOutcome::Diverged { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("divergent operation was not sealed once"));
    }
    let again_window = LeaseWindow::new(Datum::String("holder/divergent-again".into()), 120, 140)
        .map_err(gate)?
        .in_clock(domain)
        .map_err(gate)?;
    let again = OperationLifecycle::from_shared(Arc::clone(&backend))
        .with_clock(clock)
        .run(&intent, &grant, again_window, &mut performer, &mut observer)
        .map_err(gate)?;
    if !matches!(again, OperationOutcome::Diverged { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("divergent operation was replayed"));
    }
    facts(
        "divergent",
        [
            "state=divergent",
            "lease=expired",
            "replay=refused",
            "calls=1",
        ],
    )
}

fn uncertain() -> Result<Class, WorldError> {
    let run = unavailable("world/effects/uncertain")?;
    let again = reopen_unavailable(&run, "holder/uncertain-again", 30, 40)?;
    if !matches!(again, OperationOutcome::Uncertain { .. }) || run.calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("uncertain operation was replayed"));
    }
    facts(
        "uncertain",
        ["state=uncertain", "replay=refused", "calls=1"],
    )
}

fn leased() -> Result<Class, WorldError> {
    let run = verified("world/effects/leased", 10, 100)?;
    let record = lifecycle(&run.backend)
        .record(run.intent.id())
        .map_err(gate)?
        .ok_or_else(|| broken("leased record is absent"))?;
    let live = record
        .leases()
        .iter()
        .any(|lease| lease.is_live_at(50) && !lease.expired_as_of(lease.clock(), 50));
    if !live || !matches!(record.outcome(), Some(OperationOutcome::Verified { .. })) {
        return Err(broken("leased operation was not inside its window"));
    }
    facts("leased", ["state=verified", "lease=live", "tick=50"])
}

fn replayed() -> Result<Class, WorldError> {
    let completed = verified("world/effects/replay-completed", 10, 20)?;
    let completed_again = reopen(&completed, "holder/replay-completed", 20, 30)?;
    let open = unavailable("world/effects/replay-uncertain")?;
    let open_again = reopen_unavailable(&open, "holder/replay-uncertain", 30, 40)?;
    if !matches!(completed_again, OperationOutcome::Verified { .. })
        || completed.calls.load(Ordering::SeqCst) != 1
        || !matches!(open_again, OperationOutcome::Uncertain { .. })
        || open.calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("completed or uncertain operation was replayed"));
    }
    facts(
        "refused",
        [
            "completed-replay=refused",
            "uncertain-replay=refused",
            "completed-calls=1",
            "uncertain-calls=1",
        ],
    )
}

fn historical() -> Result<Class, WorldError> {
    let run = missing_ack("world/effects/historical")?;
    let record = OperationService::from_shared(run.backend)
        .record(run.intent.id())
        .map_err(gate)?
        .ok_or_else(|| broken("historical record is absent"))?;
    if !matches!(record.state(), DurableOperationState::Dispatched { .. })
        || record.receipt().is_some()
        || run.calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("historical effect was not an unresolved dispatch"));
    }
    facts(
        "held",
        [
            "state=dispatched",
            "disposition=hold",
            "success=no",
            "second-attempt=absent",
            "receipt=absent",
            "calls=1",
        ],
    )
}

struct HeldRun {
    backend: Arc<MemoryBackend>,
    intent: OperationIntent,
    grant: OperationGrant,
    calls: Arc<AtomicUsize>,
    effect: Arc<AtomicBool>,
}

fn verified(name: &str, acquired: u64, expires: u64) -> Result<HeldRun, WorldError> {
    let backend = Arc::new(MemoryBackend::new());
    let (intent, grant) = pair(name)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let effect = Arc::new(AtomicBool::new(false));
    let mut performer = AckPerformer {
        calls: Arc::clone(&calls),
        effect: Arc::clone(&effect),
    };
    let mut observer = Present {
        effect: Arc::clone(&effect),
    };
    let outcome = lifecycle(&backend)
        .run(
            &intent,
            &grant,
            window("holder/effects", acquired, expires)?,
            &mut performer,
            &mut observer,
        )
        .map_err(gate)?;
    if !matches!(outcome, OperationOutcome::Verified { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("verified operation did not dispatch once"));
    }
    Ok(HeldRun {
        backend,
        intent,
        grant,
        calls,
        effect,
    })
}

fn unavailable(name: &str) -> Result<HeldRun, WorldError> {
    let backend = Arc::new(MemoryBackend::new());
    let (intent, grant) = pair(name)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let effect = Arc::new(AtomicBool::new(false));
    let mut performer = AckPerformer {
        calls: Arc::clone(&calls),
        effect: Arc::clone(&effect),
    };
    let mut observer = Unavailable {
        effect: Arc::clone(&effect),
    };
    let outcome = lifecycle(&backend)
        .run(
            &intent,
            &grant,
            window("holder/effects", 10, 20)?,
            &mut performer,
            &mut observer,
        )
        .map_err(gate)?;
    if !matches!(outcome, OperationOutcome::Uncertain { .. }) || calls.load(Ordering::SeqCst) != 1 {
        return Err(broken("uncertain operation did not stay unproved"));
    }
    Ok(HeldRun {
        backend,
        intent,
        grant,
        calls,
        effect,
    })
}

fn missing_ack(name: &str) -> Result<HeldRun, WorldError> {
    let backend = Arc::new(MemoryBackend::new());
    let (intent, grant) = pair(name)?;
    let attempt = OperationAttempt::new(intent.id().clone(), 0).map_err(gate)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut performer = MissingPerformer {
        calls: Arc::clone(&calls),
    };
    let mut service = OperationService::from_shared(Arc::clone(&backend));
    service.resume().map_err(gate)?;
    let state = service
        .execute(&intent, &grant, &attempt, &mut performer)
        .map_err(gate)?;
    if !matches!(state, DurableOperationState::Dispatched { .. })
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(broken("missing acknowledgement was not dispatched once"));
    }
    Ok(HeldRun {
        backend,
        intent,
        grant,
        calls,
        effect: Arc::new(AtomicBool::new(false)),
    })
}

fn reopen(
    run: &HeldRun,
    holder: &str,
    acquired: u64,
    expires: u64,
) -> Result<OperationOutcome, WorldError> {
    let mut performer = AckPerformer {
        calls: Arc::clone(&run.calls),
        effect: Arc::clone(&run.effect),
    };
    let mut observer = Present {
        effect: Arc::clone(&run.effect),
    };
    lifecycle(&run.backend)
        .run(
            &run.intent,
            &run.grant,
            window(holder, acquired, expires)?,
            &mut performer,
            &mut observer,
        )
        .map_err(gate)
}

fn reopen_unavailable(
    run: &HeldRun,
    holder: &str,
    acquired: u64,
    expires: u64,
) -> Result<OperationOutcome, WorldError> {
    let mut performer = AckPerformer {
        calls: Arc::clone(&run.calls),
        effect: Arc::clone(&run.effect),
    };
    let mut observer = Unavailable {
        effect: Arc::clone(&run.effect),
    };
    lifecycle(&run.backend)
        .run(
            &run.intent,
            &run.grant,
            window(holder, acquired, expires)?,
            &mut performer,
            &mut observer,
        )
        .map_err(gate)
}

fn lifecycle(backend: &Arc<MemoryBackend>) -> OperationLifecycle<MemoryBackend> {
    OperationLifecycle::from_shared(Arc::clone(backend))
}

fn pair(name: &str) -> Result<(OperationIntent, OperationGrant), WorldError> {
    let intent = OperationIntent::new(
        name,
        Datum::String("target/effects".into()),
        Datum::String("present".into()),
        ReplayPolicy::ExactlyOnce,
    )
    .map_err(gate)?;
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("world/effects"),
        Datum::String("authority/effects".into()),
    )
    .map_err(gate)?;
    Ok((intent, grant))
}

fn window(holder: &str, acquired: u64, expires: u64) -> Result<LeaseWindow, WorldError> {
    LeaseWindow::new(Datum::String(holder.into()), acquired, expires).map_err(gate)
}

fn facts<const N: usize>(detail: &str, lines: [&str; N]) -> Result<Class, WorldError> {
    let mut owned: Vec<String> = lines.into_iter().map(str::to_owned).collect();
    owned.sort();
    let report = owned.join("\n");
    if report.contains("completion/claim-") || detail.contains("completion/claim-") {
        return Err(broken("claim label"));
    }
    let digest = hex(Sha256::digest(report.as_bytes()).as_slice());
    Ok(Class {
        detail: detail.to_owned(),
        digest,
        #[cfg(test)]
        report,
    })
}

fn gate(error: OperationError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn broken(reason: &str) -> WorldError {
    WorldError::Qualification(reason.to_owned())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

struct SharedClock {
    tick: Arc<AtomicU64>,
    domain: Datum,
}

impl LeaseClock for SharedClock {
    fn read(&self) -> Result<LeaseClockReading, OperationError> {
        Ok(LeaseClockReading {
            domain: self.domain.clone(),
            tick: self.tick.load(Ordering::SeqCst),
        })
    }
}

struct AckPerformer {
    calls: Arc<AtomicUsize>,
    effect: Arc<AtomicBool>,
}

impl LifecyclePerformer for AckPerformer {
    fn identity(&self) -> Datum {
        Datum::String("performer/effects".into())
    }

    fn perform(&mut self, dispatch: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.effect.store(true, Ordering::SeqCst);
        LifecyclePerformerResponse::Receipt(Datum::String(dispatch.id().to_string()))
    }
}

struct ExpiryPerformer {
    calls: Arc<AtomicUsize>,
    tick: Arc<AtomicU64>,
}

impl LifecyclePerformer for ExpiryPerformer {
    fn identity(&self) -> Datum {
        Datum::String("performer/effects".into())
    }

    fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.tick.store(120, Ordering::SeqCst);
        LifecyclePerformerResponse::AcknowledgementMissing
    }
}

struct Present {
    effect: Arc<AtomicBool>,
}

impl PostconditionObserver for Present {
    fn identity(&self) -> Datum {
        Datum::String("observer/effects".into())
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

struct Absent;

impl PostconditionObserver for Absent {
    fn identity(&self) -> Datum {
        Datum::String("observer/effects".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        PostconditionResponse::NotSatisfied {
            observed: Datum::String("absent".into()),
            evidence: Datum::String("observer/absent".into()),
        }
    }
}

struct Unavailable {
    effect: Arc<AtomicBool>,
}

impl PostconditionObserver for Unavailable {
    fn identity(&self) -> Datum {
        Datum::String("observer/effects".into())
    }

    fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
        if self.effect.load(Ordering::SeqCst) {
            PostconditionResponse::Unavailable {
                reason: Datum::String("unproved".into()),
            }
        } else {
            PostconditionResponse::NotSatisfied {
                observed: Datum::String("absent".into()),
                evidence: Datum::String("observer/absent".into()),
            }
        }
    }
}

struct MissingPerformer {
    calls: Arc<AtomicUsize>,
}

impl OperationPerformer for MissingPerformer {
    fn perform(&mut self, _: &OperationDispatch) -> PerformerResponse {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PerformerResponse::AcknowledgementMissing
    }
}
