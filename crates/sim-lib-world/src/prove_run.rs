// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One cold pass over the sealed catalog.
//!
//! Every selected leaf is projected, then the whole pass is executed as one
//! roadmap proof catalog. A failed leaf stays in the denominator as a gap and
//! does not abort the remaining leaves. The owner-command count is the number
//! of command leaves in that executed catalog.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::FactId;
use sim_kernel::Datum;

use crate::WorldError;
use crate::provider;

use super::ColdProof;
use super::{BoundFacts, leaf};

/// One judged leaf.
pub(super) struct Run {
    /// Whether the roadmap executor passed the leaf.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) passed: bool,
    /// Digest of the leaf name, proof code, verdict, and projected bytes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) digest: String,
    /// Projected text retained with the leaf.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) capture: String,
    /// Effect ids present on the executor receipt.
    pub(super) effects: u64,
    /// Launcher identities present on the executor receipt.
    pub(super) subprocesses: u64,
    /// Journal effect receipts the command path would append.
    pub(super) journal_records: usize,
}

/// Input change applied to an otherwise sealed cold run.
///
/// The sealed path uses [`Disturbance::none`]. A hidden read or an empty fact
/// applies to every pass. A substitute program applies only to the later
/// passes. The baseline keeps the sealed program.
#[derive(Clone, Debug, Default)]
pub(crate) struct Disturbance {
    /// Leaf that also reads `hidden_fact`.
    pub(crate) hidden_leaf: Option<String>,
    /// Fact that is not a declared dependency of `hidden_leaf`.
    pub(crate) hidden_fact: Option<String>,
    /// Replacement proof-code text for named leaves.
    pub(crate) code_for: BTreeMap<String, String>,
    /// Fact whose raw bytes are empty for this run.
    pub(crate) empty_fact: Option<String>,
}

impl Disturbance {
    /// The sealed subject, with no extra read and no substitute code.
    pub(crate) fn none() -> Self {
        Self::default()
    }
}

struct Prepared {
    input: leaf::LeafInput,
    datums: Vec<Datum>,
}

pub(super) fn execute(disturbance: Disturbance) -> Result<ColdProof, WorldError> {
    let batch = crate::assay::read_semantics()?;
    let bound = super::bind()?;
    leaf::admit_offered(&[])?;
    let mut requests = batch.requests;
    let baseline_rows = prepare_pass(
        &bound,
        &disturbance,
        false,
        crate::assay::baseline_bytes,
        &mut requests,
    )?;
    let codes = stored_codes(&bound, &disturbance);
    let baseline_done = finish(baseline_rows)?;
    let mut failures = Vec::new();
    let mut captures = BTreeMap::new();
    let mut baseline = BTreeMap::new();
    for (name, outcome) in &baseline_done.runs {
        retain(&mut failures, &mut captures, &mut baseline, name, outcome);
    }
    let mut deltas = BTreeMap::new();
    for delta in &batch.deltas {
        let rows = prepare_pass(
            &bound,
            &disturbance,
            true,
            |fact| crate::assay::variant_bytes(delta.delta, fact),
            &mut requests,
        )?;
        for row in &rows {
            let Some(code) = codes.get(&row.input.name) else {
                return Err(WorldError::Qualification(format!(
                    "proof program for {} is missing",
                    row.input.name
                )));
            };
            if code != &row.input.code {
                return Err(WorldError::Qualification(format!(
                    "proof program for {} changed between passes",
                    row.input.name
                )));
            }
        }
        let done = finish(rows)?;
        let mut digests = BTreeMap::new();
        let mut captures = BTreeMap::new();
        for (name, outcome) in &done.runs {
            if !outcome.passed && !failures.contains(name) {
                failures.push(name.clone());
            }
            digests.insert(name.clone(), outcome.digest.clone());
            captures.insert(name.clone(), outcome.capture.clone());
        }
        deltas.insert(
            delta.delta.to_owned(),
            super::DeltaCold {
                changed: delta.changed.clone(),
                revision: delta.revision_delta,
                digests,
                captures,
            },
        );
    }
    let leaves: Vec<String> = bound.facts.keys().cloned().collect();
    let edges = bound.facts.values().map(Vec::len).sum::<usize>();
    if leaves.is_empty() || deltas.len() != super::DELTAS.len() {
        return Err(WorldError::Qualification(
            "commissioned assay is empty".to_owned(),
        ));
    }
    Ok(ColdProof {
        subject: batch.subject,
        leaves,
        requests,
        edges: u64::try_from(edges).unwrap_or(u64::MAX),
        baseline,
        facts: bound
            .facts
            .iter()
            .map(|(leaf_name, facts)| {
                (
                    leaf_name.clone(),
                    facts.iter().map(|fact| fact.as_str().to_owned()).collect(),
                )
            })
            .collect(),
        failures,
        captures,
        codes,
        owner_commands: baseline_done.owner_commands,
        hidden_leaf: disturbance.hidden_leaf,
        hidden_fact: disturbance.hidden_fact,
        empty_fact: disturbance.empty_fact,
        deltas,
        kept: BTreeSet::new(),
        claims: BTreeMap::new(),
    })
}

/// Recomputes the sealed baseline program and each retained later program.
///
/// One baseline pass and one pass per frozen delta use the catalog shape of a
/// cold run. The baseline pass runs the sealed program. Later passes run the
/// program stored for that leaf. The returned count is the semantic-provider
/// reads and the proof reads that comparison spent.
/// The body is admitted only when its captures, digests, changed facts,
/// revisions, request count, obligation facts, and failure set match.
pub(super) fn authentic(proof: &ColdProof) -> Result<Option<u64>, WorldError> {
    let bound = super::bind()?;
    let batch = crate::assay::read_semantics()?;
    if !same_facts(proof, &bound) || !same_changes(proof, &batch) {
        return Ok(None);
    }
    if bound
        .facts
        .keys()
        .any(|leaf| !proof.codes.contains_key(leaf))
    {
        return Ok(None);
    }
    let disturbance = Disturbance {
        hidden_leaf: proof.hidden_leaf.clone(),
        hidden_fact: proof.hidden_fact.clone(),
        code_for: proof.codes.clone(),
        empty_fact: proof.empty_fact.clone(),
    };
    let mut requests = batch.requests;
    let mut failed = BTreeSet::new();
    let baseline = finish(prepare_pass(
        &bound,
        &disturbance,
        false,
        crate::assay::baseline_bytes,
        &mut requests,
    )?)?;
    if !record_pass(proof, None, &baseline.runs, &mut failed) {
        return Ok(None);
    }
    for delta in &batch.deltas {
        let done = finish(prepare_pass(
            &bound,
            &disturbance,
            true,
            |fact| crate::assay::variant_bytes(delta.delta, fact),
            &mut requests,
        )?)?;
        if !record_pass(proof, Some(delta.delta), &done.runs, &mut failed) {
            return Ok(None);
        }
    }
    if requests != proof.requests || failed != proof.failures.iter().cloned().collect() {
        return Ok(None);
    }
    Ok(Some(requests))
}

fn same_facts(proof: &ColdProof, bound: &BoundFacts) -> bool {
    proof.facts.len() == bound.facts.len()
        && proof.facts.iter().all(|(leaf, names)| {
            bound.facts.get(leaf).is_some_and(|facts| {
                facts.len() == names.len()
                    && facts
                        .iter()
                        .zip(names)
                        .all(|(fact, name)| fact.as_str() == name)
            })
        })
}

fn same_changes(proof: &ColdProof, batch: &crate::assay::SemanticBatch) -> bool {
    batch.deltas.len() == proof.deltas.len()
        && batch.deltas.iter().all(|delta| {
            proof.deltas.get(delta.delta).is_some_and(|stored| {
                stored.changed == delta.changed && stored.revision == delta.revision_delta
            })
        })
}

fn record_pass(
    proof: &ColdProof,
    delta: Option<&str>,
    runs: &[(String, Run)],
    failed: &mut BTreeSet<String>,
) -> bool {
    for (name, outcome) in runs {
        let (digest, capture) = if let Some(delta) = delta {
            let Some(stored) = proof.deltas.get(delta) else {
                return false;
            };
            let Some(digest) = stored.digests.get(name) else {
                return false;
            };
            let Some(capture) = stored.captures.get(name) else {
                return false;
            };
            (digest.as_str(), capture.as_str())
        } else {
            let Some(digest) = proof.baseline.get(name) else {
                return false;
            };
            let Some(capture) = proof.captures.get(name) else {
                return false;
            };
            (digest.as_str(), capture.as_str())
        };
        if outcome.digest != digest || outcome.capture != capture {
            return false;
        }
        if !outcome.passed {
            failed.insert(name.clone());
        }
    }
    true
}

fn retain(
    failures: &mut Vec<String>,
    captures: &mut BTreeMap<String, String>,
    baseline: &mut BTreeMap<String, String>,
    leaf_name: &str,
    outcome: &Run,
) {
    if !outcome.passed {
        failures.push(leaf_name.to_owned());
    }
    captures.insert(leaf_name.to_owned(), outcome.capture.clone());
    baseline.insert(leaf_name.to_owned(), outcome.digest.clone());
}

fn stored_codes(bound: &BoundFacts, disturbance: &Disturbance) -> BTreeMap<String, String> {
    bound
        .facts
        .iter()
        .map(|(leaf_name, facts)| {
            (
                leaf_name.clone(),
                program_for(disturbance, true, leaf_name, facts),
            )
        })
        .collect()
}

fn program_for(
    disturbance: &Disturbance,
    replaced: bool,
    leaf_name: &str,
    facts: &[FactId],
) -> String {
    if replaced && let Some(code) = disturbance.code_for.get(leaf_name) {
        code.clone()
    } else {
        leaf::code_for(facts.iter().map(|fact| fact.as_str())).to_owned()
    }
}

fn prepare_pass(
    bound: &BoundFacts,
    disturbance: &Disturbance,
    replaced: bool,
    raw: impl Fn(&str) -> String,
    requests: &mut u64,
) -> Result<Vec<Prepared>, WorldError> {
    let mut rows = Vec::with_capacity(bound.facts.len());
    for (leaf_name, facts) in &bound.facts {
        let mut executed = facts.clone();
        if disturbance.hidden_leaf.as_deref() == Some(leaf_name)
            && let Some(extra) = &disturbance.hidden_fact
        {
            executed.push(FactId::new(extra.clone())?);
        }
        let code = program_for(disturbance, replaced, leaf_name, facts);
        rows.push(prepare(
            bound,
            leaf_name,
            &executed,
            |fact| raw_bytes(disturbance, fact, &raw),
            &code,
            requests,
        )?);
    }
    Ok(rows)
}

fn raw_bytes(disturbance: &Disturbance, fact: &str, raw: impl Fn(&str) -> String) -> String {
    if disturbance.empty_fact.as_deref() == Some(fact) {
        String::new()
    } else {
        raw(fact)
    }
}

/// Reruns one retained leaf with its hidden read, empty fact, and program.
pub(super) fn project_retained(
    proof: &ColdProof,
    leaf_name: &str,
    delta: &str,
    requests: &mut u64,
) -> Result<Run, WorldError> {
    let bound = super::bind()?;
    let Some(names) = proof.facts.get(leaf_name) else {
        return Err(WorldError::Qualification(format!(
            "unavailable-proof {leaf_name}"
        )));
    };
    let mut facts = names
        .iter()
        .map(|fact| FactId::new(fact.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    if proof.hidden_leaf.as_deref() == Some(leaf_name)
        && let Some(extra) = &proof.hidden_fact
    {
        facts.push(FactId::new(extra.clone())?);
    }
    let Some(code) = proof.codes.get(leaf_name) else {
        return Err(WorldError::Qualification(format!(
            "proof program for {leaf_name} is missing"
        )));
    };
    let empty = proof.empty_fact.clone();
    project(
        &bound,
        leaf_name,
        &facts,
        |fact| {
            if empty.as_deref() == Some(fact) {
                String::new()
            } else {
                crate::assay::variant_bytes(delta, fact)
            }
        },
        code,
        requests,
    )
}

pub(super) fn project(
    bound: &BoundFacts,
    leaf_name: &str,
    facts: &[FactId],
    raw: impl Fn(&str) -> String,
    code: &str,
    requests: &mut u64,
) -> Result<Run, WorldError> {
    let prepared = prepare(bound, leaf_name, facts, raw, code, requests)?;
    let done = finish(vec![prepared])?;
    done.runs
        .into_iter()
        .next()
        .map(|(_, run)| run)
        .ok_or_else(|| WorldError::Qualification(format!("unavailable-proof {leaf_name}")))
}

fn prepare(
    bound: &BoundFacts,
    leaf_name: &str,
    facts: &[FactId],
    raw: impl Fn(&str) -> String,
    code: &str,
    requests: &mut u64,
) -> Result<Prepared, WorldError> {
    let engine = sim_incremental_core::projection::ProjectionEngine::new(
        &bound.registry,
        &bound.shape,
        &bound.closure,
    );
    let mut names = Vec::with_capacity(facts.len());
    let mut projected = Vec::with_capacity(facts.len());
    let mut canonical = Vec::with_capacity(facts.len());
    let mut datums = Vec::with_capacity(facts.len());
    for fact in facts {
        let raw_text = raw(fact.as_str());
        let datum = crate::assay::project_fact(
            &engine,
            &bound.package,
            &bound.shape,
            fact,
            &raw_text,
            requests,
        )?;
        projected.push(projected_text(&datum)?);
        canonical.push(leaf::independent_canonical(&raw_text));
        names.push(fact.as_str().to_owned());
        datums.push(datum);
    }
    Ok(Prepared {
        input: leaf::LeafInput {
            name: leaf_name.to_owned(),
            code: code.to_owned(),
            facts: names,
            projected,
            canonical,
        },
        datums,
    })
}

struct Finished {
    runs: Vec<(String, Run)>,
    #[cfg_attr(not(test), allow(dead_code))]
    owner_commands: u64,
}

fn finish(rows: Vec<Prepared>) -> Result<Finished, WorldError> {
    let inputs = rows.iter().map(|row| row.input.clone()).collect::<Vec<_>>();
    let union = leaf::execute_union(&inputs)?;
    if union.leaves.len() != rows.len() {
        return Err(WorldError::Qualification(
            "catalog execution did not cover the sealed union".to_owned(),
        ));
    }
    let mut runs = Vec::with_capacity(rows.len());
    for (row, executed) in rows.iter().zip(&union.leaves) {
        if executed.name != row.input.name {
            return Err(WorldError::Qualification(format!(
                "catalog leaf {} did not match {}",
                executed.name, row.input.name
            )));
        }
        runs.push((row.input.name.clone(), digest_of(row, executed)?));
    }
    Ok(Finished {
        runs,
        owner_commands: union.owner_commands,
    })
}

fn digest_of(row: &Prepared, executed: &leaf::Executed) -> Result<Run, WorldError> {
    let capture = row
        .input
        .facts
        .iter()
        .zip(&row.input.projected)
        .map(|(name, text)| format!("{name} {text}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut hashed = Vec::with_capacity(row.datums.len() + 5);
    hashed.push(Datum::String(row.input.name.clone()));
    hashed.push(Datum::String(row.input.code.clone()));
    hashed.push(Datum::String(
        if executed.passed { "pass" } else { "fail" }.to_owned(),
    ));
    hashed.push(Datum::String(executed.semantic_detail.clone()));
    hashed.push(Datum::String(capture.clone()));
    hashed.extend(row.datums.iter().cloned());
    let id = provider::content_id(Datum::Vector(hashed))?;
    Ok(Run {
        passed: executed.passed,
        digest: hex(&id.bytes),
        capture,
        effects: executed.effects,
        subprocesses: executed.subprocesses,
        journal_records: executed.journal_records,
    })
}

/// One catalog attempt observed from the executor receipt.
pub(crate) struct Boundary {
    /// Executor receipts returned for the leaf.
    pub(crate) attempts: usize,
    /// Effect ids present on those receipts.
    pub(crate) effects: u64,
    /// Launcher identities present on those receipts.
    pub(crate) subprocesses: u64,
    /// Journal effect receipts those attempts would append.
    pub(crate) journal_records: usize,
}

/// Executes one sealed leaf through the catalog and reads the receipt.
pub(crate) fn attempt_leaf(leaf_name: &str, delta: &str) -> Result<Boundary, WorldError> {
    let bound = super::bind()?;
    let facts = bound
        .facts
        .get(leaf_name)
        .cloned()
        .ok_or_else(|| WorldError::Qualification(format!("unavailable-proof {leaf_name}")))?;
    let code = leaf::code_for(facts.iter().map(|fact| fact.as_str())).to_owned();
    let mut requests = 0u64;
    let outcome = project(
        &bound,
        leaf_name,
        &facts,
        |fact| crate::assay::variant_bytes(delta, fact),
        &code,
        &mut requests,
    )?;
    Ok(Boundary {
        attempts: 1,
        effects: outcome.effects,
        subprocesses: outcome.subprocesses,
        journal_records: outcome.journal_records,
    })
}

#[cfg(test)]
pub(crate) fn proof_implementation() -> &'static str {
    leaf::proof_implementation()
}

#[cfg(test)]
pub(crate) fn subject_material() -> Result<String, WorldError> {
    Ok(super::subject_rows(&super::bind()?)?.join("\n"))
}

#[cfg(test)]
pub(crate) fn open_view(text: &str) -> Result<Option<super::ColdProof>, WorldError> {
    Ok(super::view::open(text)?.map(|opened| opened.proof))
}

#[cfg(test)]
pub(crate) fn delta_capture<'a>(
    proof: &'a super::ColdProof,
    delta: &str,
    leaf: &str,
) -> Option<&'a str> {
    proof
        .deltas
        .get(delta)?
        .captures
        .get(leaf)
        .map(String::as_str)
}

#[cfg(test)]
pub(crate) fn delta_digest<'a>(
    proof: &'a super::ColdProof,
    delta: &str,
    leaf: &str,
) -> Option<&'a str> {
    proof
        .deltas
        .get(delta)?
        .digests
        .get(leaf)
        .map(String::as_str)
}

fn projected_text(value: &Datum) -> Result<String, WorldError> {
    let Datum::Node { fields, .. } = value else {
        return Err(WorldError::Qualification(
            "projected value is not a semantic node".to_owned(),
        ));
    };
    let Some((_, Datum::Vector(facts))) =
        fields.iter().find(|(name, _)| name.to_string() == "facts")
    else {
        return Err(WorldError::Qualification(
            "projected value has no facts".to_owned(),
        ));
    };
    let mut texts = Vec::new();
    for fact in facts {
        let Datum::Node { fields, .. } = fact else {
            return Err(WorldError::Qualification(
                "projected fact is not a node".to_owned(),
            ));
        };
        let Some((_, Datum::String(text))) =
            fields.iter().find(|(name, _)| name.to_string() == "value")
        else {
            return Err(WorldError::Qualification(
                "projected fact has no value".to_owned(),
            ));
        };
        texts.push(text.clone());
    }
    if texts.is_empty() {
        return Err(WorldError::Qualification(
            "projected value has no facts".to_owned(),
        ));
    }
    Ok(texts.join("\n"))
}

/// Recomputes one baseline digest under a substitute proof-code text.
#[cfg(test)]
pub(crate) fn digest_for_code(leaf_name: &str, code: &str) -> Result<String, WorldError> {
    digest_under(leaf_name, code, crate::assay::baseline_bytes)
}

/// Recomputes one delta digest under a substitute proof-code text.
#[cfg(test)]
pub(crate) fn digest_for_delta(
    leaf_name: &str,
    delta: &str,
    code: &str,
) -> Result<String, WorldError> {
    let delta = delta.to_owned();
    digest_under(leaf_name, code, move |fact| {
        crate::assay::variant_bytes(&delta, fact)
    })
}

#[cfg(test)]
fn digest_under(
    leaf_name: &str,
    code: &str,
    raw: impl Fn(&str) -> String,
) -> Result<String, WorldError> {
    let bound = super::bind()?;
    let facts = bound
        .facts
        .get(leaf_name)
        .ok_or_else(|| WorldError::Qualification(format!("unavailable-proof {leaf_name}")))?;
    Ok(project(&bound, leaf_name, facts, raw, code, &mut 0)?.digest)
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
