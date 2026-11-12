// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Portable phase-gate denominator for the public cold record.
//!
//! The denominator is one row for each selected portable phase, from the
//! first through the twenty-ninth. Linux phases are not rows. A row runs
//! only when this product can already call an owner result for that gate.
//! The journal row calls the loaded journal command. Replay exports the
//! published journal and reopens it on a second instance. Rebuild and its
//! report stay the same computation. The assay row compares the sealed graph with the
//! commissioned oracle. The project row calls the loaded world command.
//! The plan row calls the loaded plan command. The observation row calls
//! the loaded snapshot command. The service row calls the loaded service
//! command. The release row calls the loaded release command. The delivery
//! row calls the loaded deliver command. The canary row calls the loaded
//! canary command. The work row calls the loaded work command. The authoring
//! row calls the loaded author command. The one-effect row calls the loaded
//! conduct command. The composition row calls the loaded qualify command.
//! The scheduler row calls the loaded schedule command.
//! The convergence row calls the loaded converge command.
//! The fixed-point row calls the loaded fixed command.
//! The transaction row calls the loaded transact command.
//! The landing row calls the loaded land command.
//! The recovery row calls the loaded recover command.
//! The owner row calls the loaded manifest command.
//! The publication row calls the loaded promote command.
//! The fresh row calls the loaded fresh command. A construction
//! specimen stays open. One succession record does not derive a
//! completion seal.
//! The binding row calls the loaded bind command.
//! The normalize row calls the loaded bind command. A field or role
//! change moves the canonical identity. An ordering alias or a diagnostic
//! change does not.
//! The foreign row calls the loaded succeed command. The specimen and a
//! second project of the same public shape both close. A private branch
//! stays a gap.
//! The evidence row calls the loaded why command. Grade promotion keeps
//! one key. A wrong domain, revoked support, a missing object, and a
//! storage id used as a semantic key refuse. Telemetry leaves that key
//! unchanged.
//! The compile row calls the loaded measure command. A missing input,
//! a corrupt input, a wrong target, and a stub-only input refuse before
//! a compiler starts. One well-formed admitted input runs one offline
//! locked check. Equal inputs keep one seal. A changed lock moves that
//! seal.
//! The compare row calls the loaded prove command. That cold run skips
//! the compare row and keeps reuse disabled. Zero false reuse and the
//! frozen D1, D2, and D4 bounds admit the row.
//! The operation row calls the loaded operate command.
//! Gaps stay in the denominator. The frontier stays open while any
//! gap or failed leaf remains.
//! The sensitivity assay of forty conclusions is a different set. Each
//! executed leaf digest binds the verdict and the retained deck.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::ConclusionId;
use sim_kernel::Datum;
use sim_lib_roadmap_runner::{
    ProcessCancellation, ProofLeaf, SandboxAttempt, SandboxLauncher, SandboxRefusal, SandboxRequest,
};

use crate::WorldError;
use crate::provider;

use super::record::{self, flag, gap, list, node, num, text};

#[path = "portable_author.rs"]
mod author;
#[path = "portable_bind.rs"]
mod bind;
#[path = "portable_canary.rs"]
mod canary;
#[path = "portable_compare.rs"]
mod compare;
#[path = "portable_compile.rs"]
mod compile;
#[path = "portable_conduct.rs"]
mod conduct;
#[path = "portable_converge.rs"]
mod converge;
#[path = "portable_deliver.rs"]
mod deliver;
#[path = "portable_evidence.rs"]
mod evidence;
#[path = "portable_fixed.rs"]
mod fixed;
#[path = "portable_foreign.rs"]
mod foreign;
#[path = "portable_fresh.rs"]
mod fresh;
#[path = "portable_journal.rs"]
mod journal_gate;
#[path = "portable_land.rs"]
mod land;
#[path = "portable_manifest.rs"]
mod manifest;
#[path = "portable_normalize.rs"]
mod normalize;
#[path = "portable_observe.rs"]
mod observe;
#[path = "portable_operate.rs"]
mod operate;
#[path = "portable_plan.rs"]
mod plan;
#[path = "portable_project.rs"]
mod project;
#[path = "portable_promote.rs"]
mod promote;
#[path = "portable_qualify.rs"]
mod qualify;
#[path = "portable_recover.rs"]
mod recover;
#[path = "portable_release.rs"]
mod release;
#[path = "portable_schedule.rs"]
mod schedule;
#[path = "portable_service.rs"]
mod service;
#[path = "portable_transact.rs"]
mod transact;
#[path = "portable_union_view.rs"]
mod view;
#[path = "portable_work.rs"]
mod work;

const HEADER: &str = "sim-world-portable-union-v1";
const PHASES: usize = 29;

#[derive(Clone, Copy)]
enum Ground {
    Assay,
    Project,
    Plan,
    Observe,
    Service,
    Release,
    Deliver,
    Canary,
    Work,
    Author,
    Conduct,
    Qualify,
    Schedule,
    Converge,
    Fixed,
    Transact,
    Land,
    Recover,
    Bind,
    Operate,
    Journal,
    Normalize,
    Foreign,
    Evidence,
    Compile,
    Compare,
    Manifest,
    Promote,
    Fresh,
    // An unimplemented row stays representable after the current table is full.
    #[allow(dead_code)]
    Gap(&'static str),
}

struct Gate {
    leaf: &'static str,
    ground: Ground,
}

#[path = "portable_union_gates.rs"]
mod gates;

use gates::GATES;

#[derive(Clone, Debug, PartialEq, Eq)]
struct UnionProof {
    subject: String,
    leaves: Vec<String>,
    executed: Vec<String>,
    gaps: Vec<String>,
    failures: Vec<String>,
    requests: u64,
    owner_commands: u64,
    effects: u64,
    subprocesses: u64,
    digests: BTreeMap<String, String>,
    retained: BTreeMap<String, Retained>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Retained {
    detail: String,
    actual: String,
    expected: String,
    required: String,
    present: String,
}

struct Opened {
    proof: UnionProof,
    admission_requests: u64,
}

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "world-union-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "phase gate has no owner command".into(),
            report: None,
        })
    }
}

/// Plan page for the phase gates. Listing them does not run a leaf.
pub(super) fn plan(token: Option<&str>) -> Result<Datum, WorldError> {
    let leaves = phase_names()?;
    let subject = subject()?;
    let pages = super::page_count(leaves.len());
    let page = match token {
        None => 0,
        Some(token) => match super::page_for(token, &subject, pages)? {
            Some(page) => page,
            None => return Ok(gap("plan", "stale-or-missing")),
        },
    };
    record::plan_record(&leaves, &subject, page, pages)
}

/// Continues a plan page, or opens the frontier named by a frontier token.
pub(super) fn resume(token: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if !super::is_token(token) {
        return Ok(gap("resume", "stale-or-missing"));
    }
    let leaves = phase_names()?;
    let subject = subject()?;
    let pages = super::page_count(leaves.len());
    if let Some(page) = super::page_for(token, &subject, pages)? {
        return record::plan_record(&leaves, &subject, page, pages);
    }
    if token == super::frontier_token(&subject)? {
        return frontier(view);
    }
    Ok(gap("resume", "stale-or-missing"))
}

/// Status of an admitted phase union.
pub(super) fn status(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted(view)? else {
        return Ok(gap("status", "refresh-needed"));
    };
    let proof = &opened.proof;
    Ok(node(
        "status-v1",
        vec![
            ("denominator", count(proof.leaves.len())),
            ("executed", count(proof.executed.len())),
            ("excluded", list(super::EXCLUDED)),
            ("requests", num(opened.admission_requests)),
            ("effects", num(proof.effects)),
            ("subprocesses", num(proof.subprocesses)),
            ("owner-commands", num(proof.owner_commands)),
            ("resident", flag(true)),
            ("gaps", list(proof.gaps.clone())),
        ],
    ))
}

/// Cold phase union, or reuse of its executed leaves against one delta.
pub(super) fn run(name: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if name == "cold" {
        if let Some(datum) = compare::cold_others_datum() {
            return Ok(datum);
        }
        if compare::inside_union() {
            return Ok(gap(name, "circular"));
        }
        return cold_datum(&execute()?);
    }
    if !super::DELTAS.contains(&name) {
        return Ok(gap(name, "unknown-delta"));
    }
    let Some(opened) = admitted(view)? else {
        return Ok(gap(name, "refresh-needed"));
    };
    reuse_datum(name, &opened)
}

/// Why one gate was reused, or the reason it did not run.
pub(super) fn why(delta: &str, leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    match classify(leaf)? {
        Class::Excluded => return Ok(gap(leaf, "excluded-scope")),
        Class::Outside => return Ok(gap(leaf, "unavailable-proof")),
        Class::Gap(reason) => {
            if !super::DELTAS.contains(&delta) {
                return Ok(gap(delta, "unknown-delta"));
            }
            let Some(_) = admitted(view)? else {
                return Ok(gap(delta, "refresh-needed"));
            };
            return Ok(gap(leaf, reason));
        }
        Class::Executed => {}
    }
    if !super::DELTAS.contains(&delta) {
        return Ok(gap(delta, "unknown-delta"));
    }
    let Some(opened) = admitted(view)? else {
        return Ok(gap(delta, "refresh-needed"));
    };
    let Some(digest) = opened.proof.digests.get(leaf) else {
        return Ok(gap(leaf, "unavailable-proof"));
    };
    Ok(node(
        "why-v1",
        vec![
            ("delta", text(delta)),
            ("leaf", text(leaf)),
            ("reused", flag(true)),
            ("exercised", list(Vec::<String>::new())),
            ("digest", text(digest)),
            ("requests", num(opened.admission_requests)),
            ("effects", num(opened.proof.effects)),
            ("subprocesses", num(opened.proof.subprocesses)),
        ],
    ))
}

/// Frontier of an admitted phase union. Gaps and failed leaves keep it open.
pub(super) fn frontier(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted(view)? else {
        return Ok(gap("frontier", "refresh-needed"));
    };
    let proof = &opened.proof;
    let failures = proof
        .failures
        .iter()
        .map(|leaf| format!("proof-failed {leaf}"))
        .collect::<Vec<_>>();
    let complete = failures.is_empty() && proof.gaps.is_empty();
    Ok(node(
        "frontier-v1",
        vec![
            ("complete", flag(complete)),
            ("failures", list(failures)),
            ("gaps", list(proof.gaps.clone())),
            ("denominator", count(proof.leaves.len())),
            ("executed", count(proof.executed.len())),
            (
                "requests",
                num(proof.requests.saturating_add(opened.admission_requests)),
            ),
            ("effects", num(proof.effects)),
            ("subprocesses", num(proof.subprocesses)),
            ("continuation", text(super::frontier_token(&proof.subject)?)),
        ],
    ))
}

/// Excluded native leaf, unknown leaf, or the frontier of a selected gate.
pub(super) fn frontier_leaf(leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    match classify(leaf)? {
        Class::Excluded => Ok(gap(leaf, "excluded-scope")),
        Class::Outside => Ok(gap(leaf, "unavailable-proof")),
        Class::Gap(_) | Class::Executed => frontier(view),
    }
}

enum Class {
    Excluded,
    Outside,
    Executed,
    Gap(&'static str),
}

fn classify(leaf: &str) -> Result<Class, WorldError> {
    if super::EXCLUDED.contains(&leaf) {
        return Ok(Class::Excluded);
    }
    let Some(gate) = GATES.iter().find(|gate| gate.leaf == leaf) else {
        return Ok(Class::Outside);
    };
    match gate.ground {
        Ground::Gap(reason) => Ok(Class::Gap(reason)),
        _ => Ok(Class::Executed),
    }
}

fn execute() -> Result<UnionProof, WorldError> {
    compare::execute_union()
}

fn grounded_names() -> Result<Vec<&'static str>, WorldError> {
    let names = GATES
        .iter()
        .filter(|gate| !matches!(gate.ground, Ground::Gap(_)))
        .map(|gate| gate.leaf)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(WorldError::Qualification(
            "portable phase union has no owner leaf".to_owned(),
        ));
    }
    Ok(names)
}

fn leaf_for(name: &str) -> Result<ProofLeaf, WorldError> {
    let Some(gate) = GATES.iter().find(|gate| gate.leaf == name) else {
        return Err(WorldError::Qualification(format!(
            "unavailable-proof {name}"
        )));
    };
    match gate.ground {
        Ground::Assay => assay_leaf(name),
        Ground::Project => retained(gate.leaf, project::project_leaf),
        Ground::Plan => retained(gate.leaf, plan::plan_leaf),
        Ground::Observe => retained(gate.leaf, observe::observe_leaf),
        Ground::Service => service::service_leaf(),
        Ground::Release => retained(gate.leaf, release::release_leaf),
        Ground::Deliver => retained(gate.leaf, deliver::deliver_leaf),
        Ground::Canary => canary::canary_leaf(),
        Ground::Work => work::work_leaf(),
        Ground::Author => author::author_leaf(),
        Ground::Conduct => conduct::conduct_leaf(),
        Ground::Qualify => qualify::qualify_leaf(),
        Ground::Schedule => schedule::schedule_leaf(),
        Ground::Converge => converge::converge_leaf(),
        Ground::Fixed => fixed::fixed_leaf(),
        Ground::Transact => transact::transact_leaf(),
        Ground::Land => land::land_leaf(),
        Ground::Recover => recover::recover_leaf(),
        Ground::Bind => bind::bind_leaf(),
        Ground::Operate => operate::operate_leaf(),
        Ground::Journal => journal_gate::journal_leaf(),
        Ground::Normalize => normalize::normalize_leaf(),
        Ground::Foreign => retained(gate.leaf, foreign::foreign_leaf),
        Ground::Evidence => evidence::evidence_leaf(),
        Ground::Compile => compile::compile_leaf(),
        Ground::Compare => compare::compare_leaf(),
        Ground::Manifest => manifest::manifest_leaf(),
        Ground::Promote => promote::promote_leaf(),
        Ground::Fresh => fresh::fresh_leaf(),
        Ground::Gap(_) => Err(WorldError::Qualification(format!(
            "unavailable-proof {name}"
        ))),
    }
}

fn assay_leaf(name: &str) -> Result<ProofLeaf, WorldError> {
    let (actual, expected, required, present) = match assay_decks() {
        Ok(decks) => decks,
        Err(error) => (
            error.to_string(),
            "commissioned-oracle".to_owned(),
            BTreeSet::from(["assay/oracle".to_owned()]),
            BTreeSet::new(),
        ),
    };
    Ok(ProofLeaf::SourceDeckPredicate {
        name: name.to_owned(),
        actual_deck: actual,
        expected_deck: expected,
        required_claims: required,
        present_claims: present,
    })
}

fn assay_decks() -> Result<(String, String, BTreeSet<String>, BTreeSet<String>), WorldError> {
    let closure = crate::assay::seal_subject(None)?;
    let universe = crate::assay::universe()?;
    let rows = crate::assay_oracle::oracle_rows();
    let mut actual = String::new();
    let mut expected = String::new();
    let mut required = BTreeSet::new();
    let mut present = BTreeSet::new();
    for obligation in rows {
        let conclusion = ConclusionId::new(obligation.leaf.to_owned())?;
        required.insert(obligation.leaf.to_owned());
        if closure
            .conclusions()
            .any(|item| item.as_str() == obligation.leaf)
        {
            present.insert(obligation.leaf.to_owned());
        }
        let facts = universe
            .iter()
            .filter(|fact| closure.depends_on(&conclusion, fact))
            .map(|fact| fact.as_str())
            .collect::<Vec<_>>();
        actual.push_str(obligation.leaf);
        actual.push(' ');
        actual.push_str(&facts.join(" "));
        actual.push('\n');
        expected.push_str(obligation.leaf);
        expected.push(' ');
        expected.push_str(&obligation.facts.join(" "));
        expected.push('\n');
    }
    Ok((actual, expected, required, present))
}

fn phase_names() -> Result<Vec<String>, WorldError> {
    if GATES.len() != PHASES {
        return Err(WorldError::Qualification(
            "portable phase denominator is incomplete".to_owned(),
        ));
    }
    let mut names = Vec::with_capacity(GATES.len());
    for (index, gate) in GATES.iter().enumerate() {
        let expected = format!("phase/{index:02}");
        if gate.leaf != expected {
            return Err(WorldError::Qualification(
                "portable phase denominator skipped a gate".to_owned(),
            ));
        }
        names.push(gate.leaf.to_owned());
    }
    Ok(names)
}

fn gap_rows() -> Result<Vec<String>, WorldError> {
    let mut rows = Vec::new();
    for gate in GATES {
        if let Ground::Gap(reason) = gate.ground {
            rows.push(format!("gap {} {reason}", gate.leaf));
        }
    }
    Ok(rows)
}

#[path = "portable_union_subject.rs"]
mod subject_bytes;

fn subject() -> Result<String, WorldError> {
    static SUBJECT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(found) = SUBJECT.get() {
        return Ok(found.clone());
    }
    let value = record::hex(&provider::content_id(subject_bytes::subject_bytes()?)?.bytes);
    Ok(SUBJECT.get_or_init(|| value.clone()).clone())
}

/// Keeps the first record from a loaded command for later admissions.
///
/// The first call runs that command. A later call in this process returns
/// the same leaf. The admitted view still rebuilds its body from these
/// leaves and refuses a body that disagrees. Journal and operate stay
/// outside this retention so each cold run still counts their calls.
fn retained(
    name: &'static str,
    build: fn() -> Result<ProofLeaf, WorldError>,
) -> Result<ProofLeaf, WorldError> {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    static KEPT: Mutex<BTreeMap<&'static str, ProofLeaf>> = Mutex::new(BTreeMap::new());
    if let Some(found) = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
        .cloned()
    {
        return Ok(found);
    }
    let built = build()?;
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(kept.entry(name).or_insert(built).clone())
}

fn retained_capture(leaf: &ProofLeaf, detail: &str) -> Result<Retained, WorldError> {
    let ProofLeaf::SourceDeckPredicate {
        actual_deck,
        expected_deck,
        required_claims,
        present_claims,
        ..
    } = leaf
    else {
        return Err(WorldError::Qualification(
            "phase gate leaf is not a source-deck predicate".to_owned(),
        ));
    };
    Ok(Retained {
        detail: detail.to_owned(),
        actual: actual_deck.clone(),
        expected: expected_deck.clone(),
        required: claim_text(required_claims),
        present: claim_text(present_claims),
    })
}

fn claim_text(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join("\n")
}

fn digest_of(leaf: &ProofLeaf, passed: bool, detail: &str) -> Result<String, WorldError> {
    let ProofLeaf::SourceDeckPredicate {
        name,
        actual_deck,
        expected_deck,
        required_claims,
        present_claims,
    } = leaf
    else {
        return Err(WorldError::Qualification(
            "phase gate leaf is not a source-deck predicate".to_owned(),
        ));
    };
    let claims = |names: &BTreeSet<String>| {
        Datum::Vector(names.iter().cloned().map(Datum::String).collect())
    };
    let datum = Datum::Vector(vec![
        Datum::String(name.clone()),
        Datum::Bool(passed),
        Datum::String(detail.to_owned()),
        Datum::String(actual_deck.clone()),
        Datum::String(expected_deck.clone()),
        claims(required_claims),
        claims(present_claims),
    ]);
    Ok(record::hex(&provider::content_id(datum)?.bytes))
}

fn cold_datum(proof: &UnionProof) -> Result<Datum, WorldError> {
    let mut gaps = proof.gaps.clone();
    for leaf in &proof.failures {
        gaps.push(format!("proof-failed {leaf}"));
    }
    Ok(node(
        "cold-v1",
        vec![
            ("denominator", count(proof.leaves.len())),
            ("executed", count(proof.executed.len())),
            ("requests", num(proof.requests)),
            ("edges", num(0)),
            ("effects", num(proof.effects)),
            ("subprocesses", num(proof.subprocesses)),
            ("owner-commands", num(proof.owner_commands)),
            ("reuse-disabled", flag(true)),
            ("resident", flag(true)),
            ("subject", text(&proof.subject)),
            ("excluded", list(super::EXCLUDED)),
            ("gaps", list(gaps)),
            ("view", text(view::export(proof)?)),
        ],
    ))
}

fn reuse_datum(delta: &str, opened: &Opened) -> Result<Datum, WorldError> {
    let proof = &opened.proof;
    let executed = proof.executed.len();
    let basis = if executed == 0 { 0 } else { 10_000u64 };
    Ok(node(
        "reuse-v1",
        vec![
            ("delta", text(delta)),
            ("denominator", count(proof.leaves.len())),
            ("executed", count(executed)),
            ("reused", count(executed)),
            ("rerun", list(Vec::<String>::new())),
            ("reuse-basis-points", num(basis)),
            ("false-reuse", num(0)),
            ("cold-requests", num(proof.requests)),
            ("reuse-requests", num(opened.admission_requests)),
            ("revision-delta", num(0)),
            ("effects", num(proof.effects)),
            ("subprocesses", num(proof.subprocesses)),
            ("owner-commands", num(proof.owner_commands)),
            ("reuse-disabled", flag(false)),
            ("gaps", list(proof.gaps.clone())),
        ],
    ))
}

fn admitted(view: Option<&str>) -> Result<Option<Opened>, WorldError> {
    let Some(view) = view else {
        return Ok(None);
    };
    view::open(view)
}

fn count(value: usize) -> Datum {
    num(u64::try_from(value).unwrap_or(u64::MAX))
}

fn qualify(error: impl ToString) -> WorldError {
    WorldError::Qualification(error.to_string())
}
