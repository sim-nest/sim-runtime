// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Portable phase-gate denominator for the public cold record.
//!
//! The denominator is one row for each selected portable phase, from the
//! first through the twenty-ninth. Linux phases are not rows. A row runs
//! only when this product can already call an owner result for that gate.
//! The journal row is a named gap: `rebuild` and `rebuild_report` are the
//! same computation. The assay row compares the sealed graph with the
//! commissioned oracle. Every other row is a named gap. Gaps stay in the
//! denominator. The frontier stays open while any gap or failed leaf remains.
//! The sensitivity assay of forty conclusions is a different set. Each
//! executed leaf digest binds the verdict and the retained deck.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::ConclusionId;
use sim_kernel::Datum;
use sim_lib_roadmap_runner::{
    ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
    SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
};

use crate::WorldError;
use crate::provider;

use super::record::{self, flag, gap, list, node, num, text};

#[path = "portable_union_view.rs"]
mod view;

const HEADER: &str = "sim-world-portable-union-v1";
const PHASES: usize = 29;

#[derive(Clone, Copy)]
enum Ground {
    Assay,
    Gap(&'static str),
}

struct Gate {
    leaf: &'static str,
    ground: Ground,
}

const GATES: &[Gate] = &[
    Gate {
        leaf: "phase/00",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/01",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/02",
        ground: Ground::Gap("same-computation"),
    },
    Gate {
        leaf: "phase/03",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/04",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/05",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/06",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/07",
        ground: Ground::Assay,
    },
    Gate {
        leaf: "phase/08",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/09",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/10",
        ground: Ground::Gap("host-read"),
    },
    Gate {
        leaf: "phase/11",
        ground: Ground::Gap("foreign-crate"),
    },
    Gate {
        leaf: "phase/12",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/13",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/14",
        ground: Ground::Gap("circular"),
    },
    Gate {
        leaf: "phase/15",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/16",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/17",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/18",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/19",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/20",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/21",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/22",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/23",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/24",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/25",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/26",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/27",
        ground: Ground::Gap("held"),
    },
    Gate {
        leaf: "phase/28",
        ground: Ground::Gap("unimplemented"),
    },
];

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
        Ground::Assay => Ok(Class::Executed),
    }
}

fn execute() -> Result<UnionProof, WorldError> {
    let leaves = phase_names()?;
    let names = grounded_names()?;
    let built = names
        .iter()
        .map(|name| leaf_for(name))
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = ProofCatalog::new(built).map_err(qualify)?;
    let launcher = RefuseLaunch;
    let cancellation = ProcessCancellation::default();
    let mut executed = Vec::with_capacity(names.len());
    let mut failures = Vec::new();
    let mut digests = BTreeMap::new();
    let mut retained = BTreeMap::new();
    let mut owner_commands = 0u64;
    let mut effects = 0u64;
    let mut subprocesses = 0u64;
    for name in &names {
        let leaf = catalog.leaf(name).map_err(qualify)?;
        if matches!(leaf, ProofLeaf::Command(_)) {
            owner_commands = owner_commands.saturating_add(1);
        }
        let receipt =
            execute_proof_leaf(&catalog, name, &launcher, &cancellation, "portable-union")
                .map_err(qualify)?;
        let work = crate::query::HostWork::observe(
            receipt.effect_id.as_deref(),
            receipt.launcher_identity.as_deref(),
        );
        effects = effects.saturating_add(work.effects());
        subprocesses = subprocesses.saturating_add(work.subprocesses());
        if receipt.disposition != ProofDisposition::Passed {
            failures.push((*name).to_owned());
        }
        let passed = receipt.disposition == ProofDisposition::Passed;
        digests.insert(
            (*name).to_owned(),
            digest_of(leaf, passed, &receipt.semantic_detail)?,
        );
        retained.insert(
            (*name).to_owned(),
            retained_capture(leaf, &receipt.semantic_detail)?,
        );
        executed.push((*name).to_owned());
    }
    Ok(UnionProof {
        subject: subject()?,
        leaves,
        executed,
        gaps: gap_rows(),
        failures,
        requests: u64::try_from(names.len()).unwrap_or(u64::MAX),
        owner_commands,
        effects,
        subprocesses,
        digests,
        retained,
    })
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

fn gap_rows() -> Vec<String> {
    GATES
        .iter()
        .filter_map(|gate| match gate.ground {
            Ground::Gap(reason) => Some(format!("gap {} {reason}", gate.leaf)),
            Ground::Assay => None,
        })
        .collect()
}

fn subject() -> Result<String, WorldError> {
    let datum = Datum::Vector(vec![
        Datum::String(include_str!("portable_union.rs").to_owned()),
        Datum::String(include_str!("portable_union_view.rs").to_owned()),
        Datum::String(include_str!("assay.rs").to_owned()),
        Datum::String(include_str!("assay_oracle.rs").to_owned()),
        Datum::String(include_str!("provider.rs").to_owned()),
        Datum::String(include_str!("../freeze/incremental-graph.rs").to_owned()),
        Datum::String(include_str!("../freeze/incremental-model.rs").to_owned()),
        Datum::String(include_str!("../freeze/runner-proof.rs.freeze").to_owned()),
        Datum::String(include_str!("../freeze/agent-net-Cargo.lock").to_owned()),
        Datum::String(include_str!("../freeze/runtime-Cargo.lock").to_owned()),
    ]);
    Ok(record::hex(&provider::content_id(datum)?.bytes))
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
