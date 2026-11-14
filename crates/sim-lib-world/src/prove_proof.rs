// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assay proof view on the loaded prove command.
//!
//! `proof` exports one cold run of the forty conclusions. A later delta opens
//! that view and compares each leaf with the sealed affected set. Keeping an
//! affected leaf, or claiming an input the leaf did not read, counts as false
//! reuse. The frontier of an admitted view names the same failures whether
//! the caller asks for the whole view or for one conclusion in it. The
//! frontier token of that view resumes it. A phase-union view stays on the
//! phase-union command.

use std::collections::BTreeSet;

use sim_kernel::Datum;

use crate::WorldError;

use super::record::{flag, gap, list, node, num, text};

pub(super) fn export_datum() -> Result<Datum, WorldError> {
    let cold = super::execute_cold()?;
    let failed = failure_lines(&cold);
    let view = super::view::export(&cold)?;
    Ok(node(
        "proof-v1",
        vec![
            ("denominator", count(cold.leaves.len())),
            ("executed", count(executed(&cold))),
            ("requests", num(cold.requests)),
            ("edges", num(cold.edges)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("owner-commands", num(cold.owner_commands)),
            ("resident", flag(true)),
            ("subject", text(cold.subject)),
            ("failures", list(failed)),
            ("view", text(view)),
        ],
    ))
}

pub(super) fn reuse_datum(delta: &str, view: &str) -> Result<Datum, WorldError> {
    let Some(opened) = super::view::open(view)? else {
        return Ok(gap(delta, "refresh-needed"));
    };
    let Some(report) = compare(&opened.proof, delta)? else {
        return Ok(gap(delta, "unknown-delta"));
    };
    Ok(node(
        "reuse-v1",
        vec![
            ("delta", text(delta)),
            ("denominator", count(opened.proof.leaves.len())),
            ("executed", count(executed(&opened.proof))),
            ("reused", count(report.reused.len())),
            ("reused-leaves", names(&report.reused)),
            ("rerun", names(&report.rerun)),
            ("reuse-basis-points", num(u64::from(report.basis))),
            ("false-reuse", num(report.false_reuse)),
            ("cold-requests", num(report.cold_requests)),
            ("reuse-requests", num(report.reuse_requests)),
            ("revision-delta", num(report.revision)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("owner-commands", num(opened.proof.owner_commands)),
            ("reuse-disabled", flag(false)),
            ("gaps", list(failure_lines(&opened.proof))),
        ],
    ))
}

pub(super) fn status(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted(view)? else {
        return Ok(gap("status", "refresh-needed"));
    };
    let proof = &opened.proof;
    Ok(node(
        "status-v1",
        vec![
            ("denominator", count(proof.leaves.len())),
            ("executed", count(executed(proof))),
            ("excluded", list(super::EXCLUDED)),
            ("requests", num(opened.admission_requests)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("owner-commands", num(proof.owner_commands)),
            ("resident", flag(true)),
            ("gaps", list(failure_lines(proof))),
        ],
    ))
}

pub(super) fn why(delta: &str, leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if !super::DELTAS.contains(&delta) {
        return Ok(gap(delta, "unknown-delta"));
    }
    let Some(opened) = admitted(view)? else {
        return Ok(gap(delta, "refresh-needed"));
    };
    if !opened.proof.leaves.iter().any(|name| name == leaf) {
        return Ok(gap(leaf, "unavailable-proof"));
    }
    let Some(report) = compare(&opened.proof, delta)? else {
        return Ok(gap(delta, "unknown-delta"));
    };
    let reused = report.reused.contains(leaf);
    let exercised = if reused {
        Vec::new()
    } else {
        opened.proof.facts.get(leaf).cloned().unwrap_or_default()
    };
    let Some(digest) = report.compared.get(leaf) else {
        return Ok(gap(leaf, "unavailable-proof"));
    };
    Ok(node(
        "why-v1",
        vec![
            ("delta", text(delta)),
            ("leaf", text(leaf)),
            ("reused", flag(reused)),
            ("false-reuse", flag(report.false_leaves.contains(leaf))),
            ("exercised", list(exercised)),
            ("digest", text(digest.as_str())),
            ("requests", num(report.reuse_requests)),
            ("effects", num(0)),
            ("subprocesses", num(0)),
        ],
    ))
}

pub(super) fn frontier(view: Option<&str>) -> Result<Datum, WorldError> {
    let Some(opened) = admitted(view)? else {
        return Ok(gap("frontier", "refresh-needed"));
    };
    let proof = &opened.proof;
    let failures = failure_lines(proof);
    let complete = failures.is_empty();
    Ok(node(
        "frontier-v1",
        vec![
            ("complete", flag(complete)),
            ("failures", list(failures)),
            ("gaps", list(Vec::<String>::new())),
            ("denominator", count(proof.leaves.len())),
            ("executed", count(executed(proof))),
            (
                "requests",
                num(proof.requests.saturating_add(opened.admission_requests)),
            ),
            ("effects", num(0)),
            ("subprocesses", num(0)),
            ("continuation", text(super::frontier_token(&proof.subject)?)),
        ],
    ))
}

pub(super) fn frontier_leaf(leaf: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if super::EXCLUDED.contains(&leaf) {
        return Ok(gap(leaf, "excluded-scope"));
    }
    let Some(opened) = admitted(view)? else {
        return Ok(gap("frontier", "refresh-needed"));
    };
    if opened.proof.leaves.iter().any(|name| name == leaf) {
        return frontier(view);
    }
    Ok(gap(leaf, "unavailable-proof"))
}

pub(super) fn resume(token: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if !super::is_token(token) {
        return Ok(gap("resume", "stale-or-missing"));
    }
    let Some(opened) = admitted(view)? else {
        return Ok(gap("resume", "refresh-needed"));
    };
    if token == super::frontier_token(&opened.proof.subject)? {
        return frontier(view);
    }
    Ok(gap("resume", "stale-or-missing"))
}

fn admitted(view: Option<&str>) -> Result<Option<super::view::Opened>, WorldError> {
    let Some(view) = view else {
        return Ok(None);
    };
    super::view::open(view)
}

fn compare(proof: &super::ColdProof, delta: &str) -> Result<Option<super::Report>, WorldError> {
    let kept = proof.kept.clone();
    let claims = proof.claims.clone();
    super::reuse(proof, delta, move |leaf, affected| super::Choice {
        rerun: affected && !kept.contains(leaf),
        extra_claim: claims.get(leaf).cloned(),
    })
}

fn executed(proof: &super::ColdProof) -> usize {
    proof.leaves.len().saturating_sub(proof.failures.len())
}

fn failure_lines(proof: &super::ColdProof) -> Vec<String> {
    proof
        .failures
        .iter()
        .map(|leaf| format!("proof-failed {leaf}"))
        .collect()
}

fn names(values: &BTreeSet<String>) -> Datum {
    list(values.iter().cloned())
}

fn count(value: usize) -> Datum {
    num(u64::try_from(value).unwrap_or(u64::MAX))
}
