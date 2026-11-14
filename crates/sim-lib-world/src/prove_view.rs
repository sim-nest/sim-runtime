// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Revision-fenced text of one cold proof.
//!
//! A later query imports this text, checks the revision against the canonical
//! body, and checks the sealed subject against the live provider. Opening the
//! body reruns each retained program against the live projection and refuses
//! captures, digests, changed facts, failures, request counts, or revisions
//! that disagree. A malformed, resigned-away, or stale body is not a proof.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::FactId;
use sim_kernel::Datum;

use crate::WorldError;
use crate::provider;

use super::{ColdProof, DeltaCold};

pub(super) const HEADER: &str = "sim-world-proof-view-v1";

pub(super) struct Opened {
    pub(super) proof: ColdProof,
    pub(super) admission_requests: u64,
}

pub(super) fn export(cold: &ColdProof) -> Result<String, WorldError> {
    let (leaves, sealed) = super::sealed_ids()?;
    if cold.leaves != leaves {
        return Err(WorldError::Qualification(
            "proof view subject does not match the cold leaves".to_owned(),
        ));
    }
    let body = body_of(cold, &sealed)?;
    seal(&body)
}

pub(super) fn open(text: &str) -> Result<Option<Opened>, WorldError> {
    let Some(body) = unsigned_body(text) else {
        return Ok(None);
    };
    let Some(stated) = revision_token(text) else {
        return Ok(None);
    };
    if stated != revision_of(&body)? {
        return Ok(None);
    }
    let Some(parsed) = parse_body(&body)? else {
        return Ok(None);
    };
    let (leaves, live) = super::sealed_ids()?;
    let assay = crate::assay::frozen_subject()?;
    if parsed.subject != live || parsed.proof.leaves != leaves || parsed.proof.subject != assay {
        return Ok(None);
    }
    let Some(admission_requests) = super::run::authentic(&parsed.proof)? else {
        return Ok(None);
    };
    Ok(Some(Opened {
        proof: parsed.proof,
        admission_requests,
    }))
}

#[cfg(test)]
pub(super) fn reseal_replaced(
    text: &str,
    from: &str,
    to: &str,
) -> Result<Option<String>, WorldError> {
    let Some(body) = unsigned_body(text) else {
        return Ok(None);
    };
    if !body.contains(from) {
        return Ok(None);
    }
    Ok(Some(seal(&body.replacen(from, to, 1))?))
}

fn seal(body: &str) -> Result<String, WorldError> {
    let digest = revision_of(body)?;
    let Some((header, rest)) = body.split_once(';') else {
        return Err(WorldError::Qualification(
            "proof view body has no records".to_owned(),
        ));
    };
    if header != HEADER {
        return Err(WorldError::Qualification(
            "proof view body has the wrong header".to_owned(),
        ));
    }
    Ok(format!("{header};revision {digest};{rest}"))
}

fn revision_of(body: &str) -> Result<String, WorldError> {
    let id = provider::content_id(Datum::String(body.to_owned()))?;
    Ok(super::record::hex(&id.bytes))
}

fn unsigned_body(text: &str) -> Option<String> {
    if text.is_empty() || text.contains(['\n', '\r']) {
        return None;
    }
    let mut revision = None;
    let mut parts = Vec::new();
    for part in text.split(';') {
        if part.is_empty() {
            return None;
        }
        if let Some(rest) = part.strip_prefix("revision ") {
            if revision.is_some() || !is_revision(rest) {
                return None;
            }
            revision = Some(rest);
            continue;
        }
        parts.push(part);
    }
    if revision.is_none() || parts.first().copied() != Some(HEADER) {
        return None;
    }
    Some(parts.join(";"))
}

fn revision_token(text: &str) -> Option<&str> {
    text.split(';').find_map(|part| {
        part.strip_prefix("revision ")
            .filter(|rest| is_revision(rest))
    })
}

fn is_revision(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn body_of(cold: &ColdProof, sealed: &str) -> Result<String, WorldError> {
    let mut lines = Vec::new();
    lines.push(HEADER.to_owned());
    push(&mut lines, "batch", &[&cold.subject])?;
    push(&mut lines, "subject", &[sealed])?;
    push(&mut lines, "requests", &[&cold.requests.to_string()])?;
    push(&mut lines, "edges", &[&cold.edges.to_string()])?;
    for leaf in &cold.leaves {
        push(&mut lines, "leaf", &[leaf])?;
    }
    for leaf in &cold.leaves {
        let Some(facts) = cold.facts.get(leaf) else {
            return Err(WorldError::Qualification(format!(
                "proof view leaf {leaf} has no facts"
            )));
        };
        for fact in facts {
            push(&mut lines, "fact", &[leaf, fact])?;
        }
    }
    for leaf in &cold.leaves {
        let Some(digest) = cold.baseline.get(leaf) else {
            return Err(WorldError::Qualification(format!(
                "proof view leaf {leaf} has no baseline"
            )));
        };
        push(&mut lines, "baseline", &[leaf, digest])?;
    }
    for leaf in &cold.leaves {
        let Some(capture) = cold.captures.get(leaf) else {
            return Err(WorldError::Qualification(format!(
                "proof view leaf {leaf} has no capture"
            )));
        };
        let encoded = hex_utf8(capture);
        push(&mut lines, "capture", &[leaf, &encoded])?;
    }
    for leaf in &cold.leaves {
        let Some(code) = cold.codes.get(leaf) else {
            return Err(WorldError::Qualification(format!(
                "proof view leaf {leaf} has no program"
            )));
        };
        let encoded = hex_utf8(code);
        push(&mut lines, "program", &[leaf, &encoded])?;
    }
    if let (Some(leaf), Some(fact)) = (&cold.hidden_leaf, &cold.hidden_fact) {
        push(&mut lines, "hidden", &[leaf, fact])?;
    }
    if let Some(fact) = &cold.empty_fact {
        push(&mut lines, "empty", &[fact])?;
    }
    for leaf in &cold.failures {
        push(&mut lines, "failure", &[leaf])?;
    }
    for leaf in &cold.kept {
        push(&mut lines, "retain", &[leaf])?;
    }
    for (leaf, fact) in &cold.claims {
        push(&mut lines, "claim", &[leaf, fact])?;
    }
    for (name, delta) in &cold.deltas {
        push(&mut lines, "delta", &[name, &delta.revision.to_string()])?;
        for fact in &delta.changed {
            push(&mut lines, "changed", &[name, fact.as_str()])?;
        }
        for (leaf, digest) in &delta.digests {
            push(&mut lines, "digest", &[name, leaf, digest])?;
        }
        for leaf in &cold.leaves {
            let Some(capture) = delta.captures.get(leaf) else {
                return Err(WorldError::Qualification(format!(
                    "proof view leaf {leaf} has no capture for {name}"
                )));
            };
            let encoded = hex_utf8(capture);
            push(&mut lines, "delta-capture", &[name, leaf, &encoded])?;
        }
    }
    Ok(lines.join(";"))
}

fn hex_utf8(text: &str) -> String {
    super::record::hex(text.as_bytes())
}

fn unhex_utf8(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let digits = text.as_bytes();
    let mut index = 0;
    while index < digits.len() {
        let hi = hex_value(digits[index])?;
        let lo = hex_value(digits[index + 1])?;
        bytes.push((hi << 4) | lo);
        index += 2;
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn push(lines: &mut Vec<String>, kind: &str, values: &[&str]) -> Result<(), WorldError> {
    let mut parts = Vec::with_capacity(values.len() + 1);
    parts.push(kind.to_owned());
    for value in values {
        token(value)?;
        parts.push((*value).to_owned());
    }
    lines.push(parts.join(" "));
    Ok(())
}

fn token(value: &str) -> Result<(), WorldError> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch == ';' || ch == '"')
    {
        return Err(WorldError::Qualification(format!(
            "proof view token {value:?} is not a single word"
        )));
    }
    Ok(())
}

struct Parsed {
    proof: ColdProof,
    subject: String,
}

fn parse_body(body: &str) -> Result<Option<Parsed>, WorldError> {
    let mut batch = None;
    let mut subject = None;
    let mut requests = None;
    let mut edges = None;
    let mut leaves = Vec::new();
    let mut facts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut baseline = BTreeMap::new();
    let mut captures = BTreeMap::new();
    let mut codes = BTreeMap::new();
    let mut hidden_leaf = None;
    let mut hidden_fact = None;
    let mut empty_fact = None;
    let mut failures = Vec::new();
    let mut kept = BTreeSet::new();
    let mut claims = BTreeMap::new();
    let mut deltas: BTreeMap<String, Draft> = BTreeMap::new();
    for part in body.split(';') {
        let mut words = part.split(' ');
        let Some(kind) = words.next() else {
            return Ok(None);
        };
        let values: Vec<&str> = words.collect();
        match kind {
            HEADER if values.is_empty() && batch.is_none() => {}
            "batch" if values.len() == 1 && batch.is_none() => batch = Some(values[0].to_owned()),
            "subject" if values.len() == 1 && subject.is_none() && is_revision(values[0]) => {
                subject = Some(values[0].to_owned());
            }
            "requests" if values.len() == 1 && requests.is_none() => {
                let Ok(value) = values[0].parse() else {
                    return Ok(None);
                };
                requests = Some(value);
            }
            "edges" if values.len() == 1 && edges.is_none() => {
                let Ok(value) = values[0].parse() else {
                    return Ok(None);
                };
                edges = Some(value);
            }
            "leaf" if values.len() == 1 && leaves.iter().all(|leaf| leaf != values[0]) => {
                leaves.push(values[0].to_owned());
            }
            "fact" if values.len() == 2 && leaves.iter().any(|leaf| leaf == values[0]) => {
                if FactId::new(values[1].to_owned()).is_err() {
                    return Ok(None);
                }
                facts
                    .entry(values[0].to_owned())
                    .or_default()
                    .push(values[1].to_owned());
            }
            "baseline"
                if values.len() == 2
                    && !baseline.contains_key(values[0])
                    && is_revision(values[1]) =>
            {
                baseline.insert(values[0].to_owned(), values[1].to_owned());
            }
            "capture" if values.len() == 2 && !captures.contains_key(values[0]) => {
                let Some(text) = unhex_utf8(values[1]) else {
                    return Ok(None);
                };
                captures.insert(values[0].to_owned(), text);
            }
            "program" if values.len() == 2 && !codes.contains_key(values[0]) => {
                let Some(text) = unhex_utf8(values[1]) else {
                    return Ok(None);
                };
                codes.insert(values[0].to_owned(), text);
            }
            "hidden" if values.len() == 2 && hidden_leaf.is_none() && hidden_fact.is_none() => {
                hidden_leaf = Some(values[0].to_owned());
                hidden_fact = Some(values[1].to_owned());
            }
            "empty" if values.len() == 1 && empty_fact.is_none() => {
                empty_fact = Some(values[0].to_owned());
            }
            "failure" if values.len() == 1 && leaves.iter().any(|leaf| leaf == values[0]) => {
                if !failures.iter().any(|leaf| leaf == values[0]) {
                    failures.push(values[0].to_owned());
                }
            }
            "retain" if values.len() == 1 && leaves.iter().any(|leaf| leaf == values[0]) => {
                if !kept.insert(values[0].to_owned()) {
                    return Ok(None);
                }
            }
            "claim" if values.len() == 2 && leaves.iter().any(|leaf| leaf == values[0]) => {
                if claims
                    .insert(values[0].to_owned(), values[1].to_owned())
                    .is_some()
                {
                    return Ok(None);
                }
            }
            "delta" if values.len() == 2 && !deltas.contains_key(values[0]) => {
                let Ok(revision) = values[1].parse() else {
                    return Ok(None);
                };
                deltas.insert(values[0].to_owned(), Draft::new(revision));
            }
            "changed" if values.len() == 2 => {
                let Some(draft) = deltas.get_mut(values[0]) else {
                    return Ok(None);
                };
                draft.changed.push(values[1].to_owned());
            }
            "digest" if values.len() == 3 && is_revision(values[2]) => {
                let Some(draft) = deltas.get_mut(values[0]) else {
                    return Ok(None);
                };
                if draft
                    .digests
                    .insert(values[1].to_owned(), values[2].to_owned())
                    .is_some()
                {
                    return Ok(None);
                }
            }
            "delta-capture" if values.len() == 3 => {
                let Some(text) = unhex_utf8(values[2]) else {
                    return Ok(None);
                };
                let Some(draft) = deltas.get_mut(values[0]) else {
                    return Ok(None);
                };
                if draft.captures.insert(values[1].to_owned(), text).is_some() {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
    }
    let (Some(batch), Some(subject), Some(requests), Some(edges)) =
        (batch, subject, requests, edges)
    else {
        return Ok(None);
    };
    if leaves.is_empty() || edges != fact_edges(&leaves, &facts) {
        return Ok(None);
    }
    if baseline.len() != leaves.len()
        || captures.len() != leaves.len()
        || codes.len() != leaves.len()
        || leaves.iter().any(|leaf| {
            !baseline.contains_key(leaf)
                || !captures.contains_key(leaf)
                || !codes.contains_key(leaf)
        })
    {
        return Ok(None);
    }
    if deltas.len() != super::DELTAS.len()
        || super::DELTAS.iter().any(|name| !deltas.contains_key(*name))
    {
        return Ok(None);
    }
    let mut cold_deltas = BTreeMap::new();
    for (name, draft) in deltas {
        let Some(changed) = fact_set(&draft.changed)? else {
            return Ok(None);
        };
        if draft.digests.len() != leaves.len()
            || draft.captures.len() != leaves.len()
            || leaves
                .iter()
                .any(|leaf| !draft.digests.contains_key(leaf) || !draft.captures.contains_key(leaf))
        {
            return Ok(None);
        }
        cold_deltas.insert(
            name,
            DeltaCold {
                changed,
                revision: draft.revision,
                digests: draft.digests,
                captures: draft.captures,
            },
        );
    }
    let mut fact_ids = BTreeMap::new();
    for leaf in &leaves {
        let Some(names) = facts.get(leaf) else {
            return Ok(None);
        };
        if names.is_empty() {
            return Ok(None);
        }
        fact_ids.insert(leaf.clone(), names.clone());
    }
    Ok(Some(Parsed {
        subject,
        proof: ColdProof {
            subject: batch,
            leaves: leaves.clone(),
            requests,
            edges,
            failures,
            captures,
            codes,
            owner_commands: command_count(&leaves, &fact_ids)?,
            hidden_leaf,
            hidden_fact,
            empty_fact,
            baseline,
            facts: fact_ids,
            deltas: cold_deltas,
            kept,
            claims,
        },
    }))
}

fn command_count(
    leaves: &[String],
    facts: &BTreeMap<String, Vec<String>>,
) -> Result<u64, WorldError> {
    let mut owner_commands = 0u64;
    for leaf in leaves {
        let names = facts.get(leaf).map(Vec::as_slice).unwrap_or(&[]);
        if super::leaf::program_for_facts(names)?.is_some() {
            owner_commands = owner_commands.saturating_add(1);
        }
    }
    Ok(owner_commands)
}

fn fact_edges(leaves: &[String], facts: &BTreeMap<String, Vec<String>>) -> u64 {
    let edges = leaves
        .iter()
        .map(|leaf| facts.get(leaf).map(Vec::len).unwrap_or(0))
        .sum::<usize>();
    u64::try_from(edges).unwrap_or(u64::MAX)
}

fn fact_set(names: &[String]) -> Result<Option<BTreeSet<FactId>>, WorldError> {
    let mut changed = BTreeSet::new();
    for name in names {
        let Ok(fact) = FactId::new(name.clone()) else {
            return Ok(None);
        };
        if !changed.insert(fact) {
            return Ok(None);
        }
    }
    Ok(Some(changed))
}

struct Draft {
    revision: u64,
    changed: Vec<String>,
    digests: BTreeMap<String, String>,
    captures: BTreeMap<String, String>,
}

impl Draft {
    fn new(revision: u64) -> Self {
        Self {
            revision,
            changed: Vec::new(),
            digests: BTreeMap::new(),
            captures: BTreeMap::new(),
        }
    }
}
