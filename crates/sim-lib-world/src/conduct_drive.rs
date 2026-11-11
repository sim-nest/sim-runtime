// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One-effect sessions for the loaded conduct command.
//!
//! Every step calls `conduct_one`. Inspection reads the retained journal
//! and transition and does not invoke the effect port.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_journal::MemoryBackend;
use sim_lib_roadmap_runner::{
    AuthorityGrant, CapabilityGrant, DirectionKind, DirectionQuestion, DirectionReply,
    DirectionRequest, DriveResult, EffectPort, EffectiveCeiling, ExecutionIdentity, InputBatch,
    Limits, OpenRequest, OwnedLimit, ReadinessPort, RoadmapRunnerService, ServiceError, WakeCause,
    WakeEvent, conduct_one,
};
use sim_roadmap_core::PhaseId;
use sim_roadmap_exec_core::{
    AttemptId, ExecutionEvent, ExecutionId, ExecutionPolicy, ExecutionPolicyId, MutationId,
    MutationPlan, Observation, PhaseRunState, Transition,
};
use sim_work_core::ResourceNeed;

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

type Runner = RoadmapRunnerService<MemoryBackend, Ready, CountEffects, Never>;

struct Session {
    service: Arc<Mutex<Runner>>,
    calls: Arc<AtomicUsize>,
}

struct Ready(PhaseId);

impl ReadinessPort for Ready {
    fn admitted_leaf(&self, _: &ContentId) -> Result<Option<PhaseId>, ServiceError> {
        Ok(Some(self.0.clone()))
    }
}

struct CountEffects(Arc<AtomicUsize>);

impl EffectPort for CountEffects {
    fn invoke(
        &self,
        identity: &ExecutionIdentity,
        phase: &PhaseId,
        _: &Transition,
        _: Option<&Symbol>,
    ) -> Result<ExecutionEvent, ServiceError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ExecutionEvent {
            execution: identity.execution.clone(),
            phase: phase.clone(),
            attempt: AttemptId::new("attempt").expect("attempt id"),
            observation: Observation::new(Symbol::new("start"), cid(20)),
        })
    }

    fn receipts(&self, _: &ExecutionIdentity) -> Result<Vec<ExecutionEvent>, ServiceError> {
        Ok(Vec::new())
    }
}

struct Never;

impl sim_lib_roadmap_runner::CancellationPort for Never {
    fn cancellation_requested(&self, _: &ExecutionId) -> bool {
        false
    }
}

impl Session {
    fn plain() -> Result<Self, ServiceError> {
        Self::open(open_request())
    }

    fn waiting() -> Result<Self, ServiceError> {
        let mut request = open_request();
        request.resources.push(ResourceNeed {
            kind: Symbol::new("slot"),
            requirement: Datum::Nil,
        });
        Self::open(request)
    }

    fn asking() -> Result<Self, ServiceError> {
        let mut request = open_request();
        request.direction = Some(question(Some(Symbol::new("decide"))));
        Self::open(request)
    }

    fn open(request: OpenRequest) -> Result<Self, ServiceError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let phase = request.phase.clone();
        let service = RoadmapRunnerService::open(
            Arc::new(MemoryBackend::new()),
            Ready(phase),
            CountEffects(Arc::clone(&calls)),
            Never,
            request,
            Limits::default(),
        )?;
        Ok(Self {
            service: Arc::new(Mutex::new(service)),
            calls,
        })
    }

    fn drive(
        &self,
        inputs: &InputBatch,
        grant: &CapabilityGrant,
    ) -> Result<DriveResult, ServiceError> {
        conduct_one(&self.service, inputs, grant)
    }

    fn invokes(&self) -> u64 {
        u64::try_from(self.calls.load(Ordering::SeqCst)).unwrap_or(u64::MAX)
    }

    fn journal_len(&self) -> Result<usize, ServiceError> {
        self.with_service(|service| Ok(service.inspect()?.journal.records.len()))
    }

    fn state(&self) -> Result<&'static str, ServiceError> {
        self.with_service(|service| Ok(state_name(service.inspect()?.transition.state)))
    }

    fn leases(&self) -> Result<&'static str, ServiceError> {
        self.with_service(|service| {
            Ok(if service.leases().is_empty() {
                "empty"
            } else {
                "held"
            })
        })
    }

    fn project(&self) -> Result<(), ServiceError> {
        self.with_service(|service| {
            let _inspection = service.inspect()?;
            Ok(())
        })
    }

    fn with_service<T>(
        &self,
        body: impl FnOnce(&Runner) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        let service = self
            .service
            .lock()
            .map_err(|_| ServiceError::Adapter("runner service lock poisoned".into()))?;
        body(&service)
    }
}

pub(crate) fn quiet() -> Result<Law, ServiceError> {
    let session = Session::plain()?;
    let before = session.journal_len()?;
    let outcome = class_of(session.drive(&InputBatch::no_change(), &CapabilityGrant::default()));
    let after = session.journal_len()?;
    let invokes = session.invokes();
    let state = session.state()?;
    Ok(Law {
        detail: format!(
            "{outcome} invokes {invokes} journal {} state {state}",
            journal_mark(before, after),
        ),
        invokes,
    })
}

pub(crate) fn one() -> Result<Law, ServiceError> {
    let session = Session::plain()?;
    let before = session.invokes();
    let outcome = class_of(session.drive(&InputBatch::conduct(), &CapabilityGrant::default()));
    let invokes = session.invokes().saturating_sub(before);
    session.project()?;
    let inspected = if session.invokes().saturating_sub(before) == invokes {
        "still"
    } else {
        "moved"
    };
    let state = session.state()?;
    Ok(Law {
        detail: format!("{outcome} invokes {invokes} inspected {inspected} state {state}"),
        invokes,
    })
}

pub(crate) fn offer() -> Result<Law, ServiceError> {
    let session = Session::waiting()?;
    let blocked = class_of(session.drive(&InputBatch::conduct(), &CapabilityGrant::default()));
    let quiet = session.drive(&InputBatch::no_change(), &CapabilityGrant::default());
    let wake = wake_mark(quiet.as_ref().ok());
    let quiet = class_of(quiet);
    let mut zero = InputBatch::no_change();
    zero.wakes.push(WakeEvent::ResourceOffer {
        kind: "slot".into(),
        amount: 0,
    });
    let _zero = class_of(session.drive(&zero, &CapabilityGrant::default()));
    let zero_invokes = session.invokes();
    let mut offer = InputBatch::no_change();
    offer.wakes.push(WakeEvent::ResourceOffer {
        kind: "slot".into(),
        amount: 2,
    });
    let resumed = class_of(session.drive(&offer, &CapabilityGrant::default()));
    let repeated = class_of(session.drive(&offer, &CapabilityGrant::default()));
    let invokes = session.invokes();
    let leases = session.leases()?;
    Ok(Law {
        detail: format!(
            "{blocked} {quiet} zero {zero_invokes} wake {wake} {resumed} {repeated} invokes {invokes} leases {leases}"
        ),
        invokes,
    })
}

pub(crate) fn direction() -> Result<Law, ServiceError> {
    let session = Session::asking()?;
    let grant = authority_grant("decide");
    let first = session.drive(&InputBatch::conduct(), &grant)?;
    let request = match first {
        DriveResult::NeedDirection { request } => request,
        other => {
            return Ok(Law {
                detail: format!("first {}", class_result(&other)),
                invokes: session.invokes(),
            });
        }
    };
    let published_len = session.journal_len()?;
    let republish = class_of(session.drive(&InputBatch::conduct(), &grant));
    let republish_journal = journal_mark(published_len, session.journal_len()?);
    let wrong = class_of(session.drive(&wrong_batch(&request), &grant));
    let advisory = class_of(session.drive(&advisory_batch(&request), &grant));
    let stale = class_of(session.drive(&stale_batch(&request), &grant));
    let held = journal_mark(published_len, session.journal_len()?);
    let missing = class_of(session.drive(
        &decision_batch(&request, "yes"),
        &CapabilityGrant::default(),
    ));
    let accepted = class_of(session.drive(&decision_batch(&request, "yes"), &grant));
    let accepted_state = session.state()?;
    let after = session.journal_len()?;
    let replay_same = class_of(session.drive(&decision_batch(&request, "yes"), &grant));
    let replay_journal = journal_mark(after, session.journal_len()?);
    let replay_new = class_of(session.drive(&decision_batch(&request, "no"), &grant));
    let ran = class_of(session.drive(&InputBatch::conduct(), &grant));
    let invokes = session.invokes();
    let state = session.state()?;
    Ok(Law {
        detail: format!(
            "need-direction {republish} journal {republish_journal} {wrong} {advisory} {stale} journal {held} {missing} {accepted} state {accepted_state} {replay_same} journal {replay_journal} {replay_new} {ran} invokes {invokes} state {state}"
        ),
        invokes,
    })
}

pub(crate) fn service_token(error: &ServiceError) -> String {
    class_error(error).to_owned()
}

fn class_of(result: Result<DriveResult, ServiceError>) -> String {
    match result {
        Ok(value) => class_result(&value).to_owned(),
        Err(error) => class_error(&error).to_owned(),
    }
}

fn class_result(result: &DriveResult) -> &'static str {
    match result {
        DriveResult::Advanced { .. } => "advanced",
        DriveResult::Quiescent { .. } => "quiescent",
        DriveResult::NeedDirection { .. } => "need-direction",
        DriveResult::Blocked { .. } => "blocked",
        DriveResult::Complete { .. } => "complete",
    }
}

fn class_error(error: &ServiceError) -> &'static str {
    match error {
        ServiceError::WrongDirection => "wrong-direction",
        ServiceError::AdvisoryDirection => "advisory-direction",
        ServiceError::StaleDirection => "stale-direction",
        ServiceError::DirectionAuthority => "direction-authority",
        ServiceError::ReplayedDirection => "replayed-direction",
        ServiceError::NotReady => "not-ready",
        ServiceError::WrongLeaf => "wrong-leaf",
        ServiceError::EffectUncertain => "uncertain",
        ServiceError::EffectOutstanding => "outstanding",
        _ => "other",
    }
}

fn wake_mark(result: Option<&DriveResult>) -> &'static str {
    match result {
        Some(DriveResult::Quiescent { wake_on }) if wake_on.contains(&WakeCause::ResourceOffer) => {
            "resource-offer"
        }
        Some(DriveResult::Quiescent { .. }) => "other",
        _ => "absent",
    }
}

fn journal_mark(before: usize, after: usize) -> &'static str {
    if before == after {
        "same"
    } else if after > before {
        "grew"
    } else {
        "shrunk"
    }
}

fn state_name(state: PhaseRunState) -> &'static str {
    match state {
        PhaseRunState::Planned => "planned",
        PhaseRunState::Running => "running",
        PhaseRunState::Reconciling => "reconciling",
        PhaseRunState::Succeeded => "succeeded",
        PhaseRunState::Failed => "failed",
        PhaseRunState::Cancelled => "cancelled",
    }
}

fn decision_batch(request: &DirectionRequest, answer: &str) -> InputBatch {
    let mut batch = InputBatch::no_change();
    batch.reply = Some(reply(request, DirectionKind::Decision, answer));
    batch
}

fn wrong_batch(request: &DirectionRequest) -> InputBatch {
    let mut batch = decision_batch(request, "yes");
    if let Some(reply) = batch.reply.as_mut() {
        reply.execution = ExecutionId::new("other").expect("execution id");
    }
    batch
}

fn advisory_batch(request: &DirectionRequest) -> InputBatch {
    let mut batch = InputBatch::no_change();
    batch.reply = Some(reply(request, DirectionKind::Advice, "maybe"));
    batch
}

fn stale_batch(request: &DirectionRequest) -> InputBatch {
    let mut batch = decision_batch(request, "yes");
    if let Some(reply) = batch.reply.as_mut() {
        reply.expected_revision = cid(99);
    }
    batch
}

fn cid(n: u8) -> ContentId {
    ContentId::from_bytes(Symbol::qualified("core", "sha256-datum-v1"), [n; 32])
}

fn open_request() -> OpenRequest {
    let policy = ExecutionPolicy {
        id: ExecutionPolicyId::new("policy").expect("policy id"),
        source_deck: cid(2),
        required_promises: Vec::new(),
        required_proofs: Vec::new(),
    };
    OpenRequest {
        authority: AuthorityGrant {
            identity: ExecutionIdentity {
                execution: ExecutionId::new("execution").expect("execution id"),
                policy: policy.id.clone(),
                roadmap: cid(1),
                source_deck: policy.source_deck.clone(),
                conduct: cid(3),
                model: cid(4),
                launcher: cid(5),
                runner: cid(6),
            },
            ceiling: EffectiveCeiling::intersect([OwnedLimit {
                owner: Symbol::new("caller"),
                unit: Symbol::new("turns"),
                amount: 4,
            }]),
            grant: cid(7),
        },
        phase: PhaseId::new("leaf").expect("phase id"),
        attempt: AttemptId::new("attempt").expect("attempt id"),
        policy,
        mutation: MutationPlan::new(
            MutationId::new("mutation").expect("mutation id"),
            Vec::new(),
            Vec::new(),
        )
        .expect("mutation plan"),
        resources: Vec::new(),
        direction: None,
        handoff: None,
    }
}

fn question(authority: Option<Symbol>) -> DirectionQuestion {
    DirectionQuestion {
        id: cid(30),
        question: "continue".into(),
        answer_kind: DirectionKind::Decision,
        required_authority: authority,
    }
}

fn reply(request: &DirectionRequest, kind: DirectionKind, answer: &str) -> DirectionReply {
    DirectionReply {
        request: request.id.clone(),
        execution: request.execution.clone(),
        expected_revision: request.expected_revision.clone(),
        answer_kind: kind,
        answer: answer.into(),
        decision: cid(40),
    }
}

fn authority_grant(name: &str) -> CapabilityGrant {
    CapabilityGrant {
        capabilities: vec![Symbol::new(name)],
    }
}
