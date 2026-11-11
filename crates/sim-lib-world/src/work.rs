// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded work command.
//!
//! Each case calls the typed packet receiver once. A later identical case
//! returns that record. Oversize input, a wrong namespace, role, copy, or
//! schema, and a packet that does not narrow return before the input port is
//! read. Incomplete output keeps the packet evidence. Malformed retries stop
//! at the attempt ceiling. A model proposal cannot carry an effect or an
//! approval. This command adds no controller. A private value is refused
//! before the reservation.

use std::cell::Cell;

use sim_conformance_core::FakeInputPort;
use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    CanonicalWork, ModelBody, ModelReturn, ModelWorker, ProposalModel, ResourceAsk, WorkReceipt,
    WorkReceiveError, WorkSource, WorkerView, receive_work,
};

use crate::WorldError;
use crate::query;

#[path = "work_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

const BODY: &[u8] = b"exact source";

/// Runs one named work case through the typed packet receiver.
///
/// # Errors
/// Returns an error when the receiver record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if leaked(case) {
        return Ok(quiet("", "private-leakage"));
    }
    if !known(case) {
        return Ok(quiet(case, "unknown-case"));
    }
    let claim = match memory::claim(case) {
        memory::Reservation::Ready(found) => return Ok(found),
        memory::Reservation::Mine(claim) => claim,
    };
    let rendered = receive(case);
    claim.commit(&rendered);
    Ok(rendered)
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "admitted"
            | "oversize"
            | "incomplete"
            | "malformed"
            | "descent"
            | "namespace"
            | "role"
            | "stale"
            | "schema"
            | "effects"
    )
}

fn receive(case: &str) -> Datum {
    let mut input = canonical(BODY);
    let drive = match case {
        "admitted" => drive(
            &input,
            &[WorkerView::Passed],
            &[offer(
                ModelBody::Proposal(b"proposal/v1\nvalue mapped".to_vec()),
                3,
            )],
        ),
        "oversize" => {
            input.budget_bytes = 3;
            drive(&input, &[], &[])
        }
        "incomplete" => drive(
            &input,
            &[WorkerView::Passed],
            &[offer(ModelBody::Incomplete(b"partial-map".to_vec()), 3)],
        ),
        "malformed" => drive(
            &input,
            &[WorkerView::Passed, WorkerView::Passed],
            &[
                offer(ModelBody::Malformed(b"first".to_vec()), 2),
                offer(ModelBody::Malformed(b"second".to_vec()), 1),
            ],
        ),
        "descent" => drive(
            &input,
            &[WorkerView::Passed],
            &[offer(ModelBody::Malformed(b"same".to_vec()), 3)],
        ),
        "namespace" => {
            input.namespace = "mirror".to_owned();
            drive(&input, &[], &[])
        }
        "role" => {
            input.source_role = "owner".to_owned();
            drive(&input, &[], &[])
        }
        "stale" => {
            input.copy = "gen-1".to_owned();
            drive(&input, &[], &[])
        }
        "schema" => {
            input.schema = "shape/other".to_owned();
            drive(&input, &[], &[])
        }
        "effects" => drive(
            &input,
            &[WorkerView::Passed],
            &[offer(
                ModelBody::Proposal(b"proposal/v1\neffect host".to_vec()),
                1,
            )],
        ),
        _ => return quiet(case, "unknown-case"),
    };
    render(case, drive)
}

struct Drive {
    result: Result<WorkReceipt, WorkReceiveError>,
    reads: usize,
    workers: usize,
    proposals: usize,
}

fn drive(input: &CanonicalWork, views: &[WorkerView], offers: &[ModelReturn]) -> Drive {
    let mut port = FakeInputPort::default();
    for source in &input.sources {
        port.insert(source.body.clone());
    }
    let worker = Views {
        views: views.to_vec(),
        calls: Cell::new(0),
    };
    let proposals = Offers {
        items: offers.to_vec(),
        next: Cell::new(0),
        calls: Cell::new(0),
    };
    let result = receive_work(input, &mut port, &worker, &proposals);
    Drive {
        result,
        reads: port.reads().len(),
        workers: worker.calls.get(),
        proposals: proposals.calls.get(),
    }
}

fn render(case: &str, drive: Drive) -> Datum {
    match drive.result {
        Ok(receipt) => success(case, receipt, drive.reads, drive.workers, drive.proposals),
        Err(error) => {
            let (reason, attempts, evidence, mirrors) = gap_parts(&error);
            gap(GapParts {
                case,
                reason,
                attempts,
                reads: drive.reads,
                evidence: &evidence,
                mirrors,
                workers: drive.workers,
                proposals: drive.proposals,
            })
        }
    }
}

fn success(
    case: &str,
    receipt: WorkReceipt,
    reads: usize,
    workers: usize,
    proposals: usize,
) -> Datum {
    let (class, work, proposal, partial, attempts) = match receipt {
        WorkReceipt::Admitted {
            work,
            proposal,
            attempts,
        } => ("admitted", work, proposal, String::new(), attempts),
        WorkReceipt::Incomplete {
            work,
            partial,
            attempts,
        } => ("incomplete", work, String::new(), utf8(&partial), attempts),
    };
    let path = work.paths.first().cloned().unwrap_or_default();
    let packet = if work.packet_id.is_empty() {
        "missing"
    } else {
        "present"
    };
    Datum::Node {
        tag: Symbol::qualified("world", "work-v1"),
        fields: vec![
            (
                Symbol::new("attempts"),
                query::count_datum(u64::from(attempts)),
            ),
            (Symbol::new("bytes"), query::count_datum(work.bytes)),
            (Symbol::new("case"), string(case)),
            (Symbol::new("class"), string(class)),
            (Symbol::new("effects"), query::count_datum(0)),
            (
                Symbol::new("files"),
                query::count_datum(u64::from(work.files)),
            ),
            (Symbol::new("packet"), string(packet)),
            (Symbol::new("partial"), string(shown(&partial))),
            (Symbol::new("path"), string(shown(&path))),
            (Symbol::new("proposal"), string(shown(&proposal))),
            (
                Symbol::new("proposal-calls"),
                query::count_datum(width(proposals)),
            ),
            (Symbol::new("reads"), query::count_datum(width(reads))),
            (Symbol::new("receiver"), string(shown(&work.receiver))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("role"), string(shown(&work.role))),
            (Symbol::new("status"), string(shown(&work.status))),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("tokens"), query::count_datum(work.tokens)),
            (
                Symbol::new("worker-calls"),
                query::count_datum(width(workers)),
            ),
        ],
    }
}

struct GapParts<'a> {
    case: &'a str,
    reason: &'a str,
    attempts: u32,
    reads: usize,
    evidence: &'a str,
    mirrors: Option<bool>,
    workers: usize,
    proposals: usize,
}

fn quiet(case: &str, reason: &str) -> Datum {
    gap(GapParts {
        case,
        reason,
        attempts: 0,
        reads: 0,
        evidence: "",
        mirrors: None,
        workers: 0,
        proposals: 0,
    })
}

fn gap(parts: GapParts<'_>) -> Datum {
    let mirrors = match parts.mirrors {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    };
    Datum::Node {
        tag: Symbol::qualified("world", "work-gap-v1"),
        fields: vec![
            (
                Symbol::new("attempts"),
                query::count_datum(u64::from(parts.attempts)),
            ),
            (Symbol::new("case"), string(shown(parts.case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("evidence"), string(shown(parts.evidence))),
            (Symbol::new("mirrors"), string(mirrors)),
            (
                Symbol::new("proposal-calls"),
                query::count_datum(width(parts.proposals)),
            ),
            (Symbol::new("reads"), query::count_datum(width(parts.reads))),
            (Symbol::new("reason"), string(shown(parts.reason))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (
                Symbol::new("worker-calls"),
                query::count_datum(width(parts.workers)),
            ),
        ],
    }
}

fn gap_parts(error: &WorkReceiveError) -> (&'static str, u32, String, Option<bool>) {
    match error {
        WorkReceiveError::Oversize => ("oversize", 0, String::new(), None),
        WorkReceiveError::Namespace { mirrors_agree } => {
            ("namespace", 0, String::new(), Some(*mirrors_agree))
        }
        WorkReceiveError::Role { mirrors_agree } => {
            ("role", 0, String::new(), Some(*mirrors_agree))
        }
        WorkReceiveError::StaleCopy { mirrors_agree } => {
            ("stale-copy", 0, String::new(), Some(*mirrors_agree))
        }
        WorkReceiveError::Schema { mirrors_agree } => {
            ("schema", 0, String::new(), Some(*mirrors_agree))
        }
        WorkReceiveError::Status { mirrors_agree } => {
            ("status", 0, String::new(), Some(*mirrors_agree))
        }
        WorkReceiveError::NoDescent { evidence } => ("no-descent", 0, utf8(evidence), None),
        WorkReceiveError::MalformedBound { attempts, evidence } => {
            ("malformed-bound", *attempts, utf8(evidence), None)
        }
        WorkReceiveError::OutputBudget { attempts, .. } => {
            ("output-budget", *attempts, String::new(), None)
        }
        WorkReceiveError::EffectsOrApproval => ("effects-or-approval", 0, String::new(), None),
        WorkReceiveError::Packet(_) => ("packet", 0, String::new(), None),
        WorkReceiveError::WorkerFailed { .. } => ("worker-failed", 0, String::new(), None),
    }
}

fn canonical(body: &[u8]) -> CanonicalWork {
    CanonicalWork {
        namespace: "work".to_owned(),
        phase: "work.1".to_owned(),
        law: "law/packet".to_owned(),
        owner_package: "sim-lib-roadmap-runner".to_owned(),
        sources: vec![WorkSource {
            surface: "api/local-adapter".to_owned(),
            path: "crates/widget/src/lib.rs".to_owned(),
            body: body.to_vec(),
        }],
        source_role: "released-dependency".to_owned(),
        target_surface: "api/projection".to_owned(),
        target_role: "funded-target".to_owned(),
        target_status: "declared".to_owned(),
        budget_bytes: 64,
        budget_files: 2,
        budget_tokens: 64,
        budget_output: 64,
        resources: vec![ResourceAsk {
            kind: "slot".to_owned(),
            amount: 1,
        }],
        copy: "gen-2".to_owned(),
        current: "gen-2".to_owned(),
        schema: "shape/proposal".to_owned(),
        mirrors_agree: true,
        max_attempts: 2,
        unresolved: 4,
        narrowed_to: 3,
    }
}

fn offer(body: ModelBody, unresolved: u64) -> ModelReturn {
    ModelReturn {
        seen: Vec::new(),
        body,
        unresolved,
    }
}

struct Views {
    views: Vec<WorkerView>,
    calls: Cell<usize>,
}

impl ModelWorker for Views {
    fn outcome(&self, _: &str) -> WorkerView {
        let index = self.calls.get();
        self.calls.set(index.saturating_add(1));
        self.views.get(index).copied().unwrap_or(WorkerView::Failed)
    }
}

struct Offers {
    items: Vec<ModelReturn>,
    next: Cell<usize>,
    calls: Cell<usize>,
}

impl ProposalModel for Offers {
    fn propose(&self, _: &str, _: u32, serialized: &[u8]) -> ModelReturn {
        self.calls.set(self.calls.get().saturating_add(1));
        let index = self.next.get();
        let Some(mut item) = self.items.get(index).cloned() else {
            return ModelReturn {
                seen: serialized.to_vec(),
                body: ModelBody::Malformed(Vec::new()),
                unresolved: 0,
            };
        };
        self.next.set(index.saturating_add(1));
        if item.seen.is_empty() {
            item.seen = serialized.to_vec();
        }
        item
    }
}

fn width(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn utf8(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap_or_default()
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}
