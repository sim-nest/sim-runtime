// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic proof plan for one sealed in-memory subject.
//! Sealing reads no provider, starts no process, and records no effect.
//! A continuation names that subject and the next page. Any other token is a
//! stale or missing gap and carries no page.

use std::collections::BTreeSet;
use std::time::Instant;

use sim_incremental_core::projection::{
    ConclusionId, Explanation, FactId, FederatedClosure, OwnerProjectionGraph,
};
use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::command::render;
use crate::product::explanation_value;
use crate::provider::content_id;
use crate::query::{self, CEILING_MS, SAMPLES, WARMUPS};

const PAGE_ROWS: usize = 2;
const MAX_CLOSURE_DEPTH: usize = 8;
const MAX_BYTES: usize = 4096;
const MAX_WORK: usize = 64;
const PLAN_KIND: &str = "world/proof-plan-v1";

/// Which public projection of one plan page the caller asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum View {
    /// Structured proof plan and proof-graph page.
    Expression,
    /// Plain-text fields of the same page.
    Text,
    /// Fixed card predicates of the same page.
    Card,
}

struct Spec {
    conclusion: &'static str,
    facts: &'static [&'static str],
    evidence: Option<&'static str>,
    status: Option<&'static str>,
    owner: Option<&'static str>,
}

const BODIES: &[(&str, Option<&str>)] = &[
    ("fact/source-a", Some("alpha")),
    ("fact/source-b", Some("beta")),
    ("fact/source-c", Some("gamma")),
    ("fact/source-gap", None),
];

const SPECS: &[Spec] = &[
    Spec {
        conclusion: "conclusion/gap-source",
        facts: &["fact/source-gap"],
        evidence: None,
        status: None,
        owner: Some("owner/alpha"),
    },
    Spec {
        conclusion: "conclusion/ready-a",
        facts: &["fact/source-a"],
        evidence: Some("evidence/current"),
        status: Some("current"),
        owner: Some("owner/alpha"),
    },
    Spec {
        conclusion: "conclusion/ready-b",
        facts: &["fact/source-b", "fact/source-c"],
        evidence: None,
        status: None,
        owner: Some("owner/alpha"),
    },
    Spec {
        conclusion: "conclusion/revoked",
        facts: &["fact/source-a"],
        evidence: Some("evidence/revoked"),
        status: Some("revoked"),
        owner: Some("owner/alpha"),
    },
    Spec {
        conclusion: "conclusion/stranger",
        facts: &[],
        evidence: None,
        status: None,
        owner: None,
    },
];

pub(crate) struct Leaf {
    pub(crate) conclusion: String,
    pub(crate) facts: Vec<String>,
    pub(crate) evidence: Option<String>,
    pub(crate) included: bool,
    pub(crate) reason: &'static str,
}

struct Explained {
    conclusion: String,
    fact: String,
    path: Vec<String>,
}

struct Subject {
    world: String,
    records: usize,
    units: usize,
    requested: Vec<String>,
    affected: Vec<String>,
    legal_ready: Vec<String>,
    leaves: Vec<Leaf>,
    gaps: Vec<(String, &'static str)>,
    floor: Vec<String>,
    explanations: Vec<Explained>,
}

pub(crate) struct MeasuredPlan {
    pub(crate) record: Datum,
    pub(crate) within_ceiling: bool,
}

/// Returns one view of a plan page. `token` of `None` is the first page.
///
/// A token is accepted only when it equals a continuation id recomputed from
/// this sealed subject. Any other token returns a gap datum.
pub(crate) fn open(view: View, token: Option<&str>) -> Result<Datum, WorldError> {
    let subject = seal()?;
    let Some(token) = token else {
        return project(view, &subject, 0);
    };
    match page_index_for_token(&subject, token)? {
        Some(index) => project(view, &subject, index),
        None => Ok(stale_gap(&subject)),
    }
}

/// Seals the planning subject once, then times the pure first page.
pub(crate) fn measure(effects: usize) -> Result<MeasuredPlan, WorldError> {
    let subject = seal()?;
    let expected = render(&project(View::Expression, &subject, 0)?);
    for _ in 1..WARMUPS {
        if render(&project(View::Expression, &subject, 0)?) != expected {
            return Err(WorldError::Qualification(
                "plan page changed during warmup".to_owned(),
            ));
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let rendered = render(&project(View::Expression, &subject, 0)?);
        let elapsed = started.elapsed();
        if rendered != expected {
            return Err(WorldError::Qualification(
                "plan page changed during sampling".to_owned(),
            ));
        }
        samples.push(elapsed);
    }
    samples.sort_unstable();
    let p95_ms =
        u64::try_from(samples[query::nearest_rank_p95(SAMPLES)].as_millis()).unwrap_or(u64::MAX);
    Ok(MeasuredPlan {
        record: query::latency_record("plan", subject.records, subject.units, p95_ms, effects),
        within_ceiling: p95_ms <= CEILING_MS,
    })
}

fn seal() -> Result<Subject, WorldError> {
    let mut universe = Vec::with_capacity(BODIES.len());
    for (fact, _) in BODIES {
        universe.push(FactId::new(*fact)?);
    }
    let mut dependencies = Vec::new();
    for spec in SPECS.iter().filter(|spec| spec.owner.is_some()) {
        let facts = spec
            .facts
            .iter()
            .map(|fact| FactId::new(*fact))
            .collect::<Result<BTreeSet<_>, _>>()?;
        dependencies.push((ConclusionId::new(spec.conclusion)?, facts));
    }
    let graph = OwnerProjectionGraph::new("owner/alpha", dependencies)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let closure = FederatedClosure::seal(universe, [graph])
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let mut leaves = Vec::with_capacity(SPECS.len());
    let mut explanations = Vec::new();
    for spec in SPECS {
        let reason = classify(spec)?;
        if let Some(owner) = spec.owner {
            let conclusion = ConclusionId::new(spec.conclusion)?;
            if closure
                .conclusions()
                .all(|sealed| sealed.as_str() != spec.conclusion)
            {
                return Err(WorldError::Qualification(format!(
                    "{owner} dropped {}",
                    spec.conclusion
                )));
            }
            let mut facts = spec.facts.to_vec();
            facts.sort_unstable();
            for fact in facts {
                let fact_id = FactId::new(fact)?;
                if !closure.depends_on(&conclusion, &fact_id) {
                    return Err(WorldError::Qualification(format!(
                        "{} lost {fact}",
                        spec.conclusion
                    )));
                }
                let explanation = closure
                    .explain(&conclusion, &fact_id)
                    .map_err(|error| WorldError::Qualification(error.to_string()))?;
                explanations.push(Explained {
                    conclusion: explanation.conclusion.as_str().to_owned(),
                    fact: explanation.fact.as_str().to_owned(),
                    path: explanation.path,
                });
            }
        } else if closure
            .conclusions()
            .any(|sealed| sealed.as_str() == spec.conclusion)
        {
            return Err(WorldError::Qualification(format!(
                "{} entered the seal without an owner",
                spec.conclusion
            )));
        }
        leaves.push(Leaf {
            conclusion: spec.conclusion.to_owned(),
            facts: spec.facts.iter().map(|fact| (*fact).to_owned()).collect(),
            evidence: spec.evidence.map(str::to_owned),
            included: reason == "legal-ready",
            reason,
        });
    }
    if explanations.len() > MAX_WORK {
        return Err(WorldError::Qualification(
            "plan explanation work exceeded the query budget".to_owned(),
        ));
    }
    explanations.sort_by(|left, right| {
        (&left.conclusion, &left.fact).cmp(&(&right.conclusion, &right.fact))
    });
    leaves.sort_by(|left, right| left.conclusion.cmp(&right.conclusion));
    let mut legal: Vec<&Leaf> = leaves.iter().filter(|leaf| leaf.included).collect();
    legal.sort_by(|left, right| legal_order(left, right));
    let requested: Vec<String> = leaves.iter().map(|leaf| leaf.conclusion.clone()).collect();
    let affected = closure
        .affected(std::iter::empty())
        .iter()
        .map(|conclusion| conclusion.as_str().to_owned())
        .collect();
    let gaps = leaves
        .iter()
        .filter(|leaf| !leaf.included)
        .map(|leaf| (leaf.conclusion.clone(), leaf.reason))
        .collect();
    Ok(Subject {
        world: hex_id(subject_datum())?,
        records: BODIES.len(),
        units: SPECS.len(),
        legal_ready: legal
            .into_iter()
            .map(|leaf| leaf.conclusion.clone())
            .collect(),
        floor: requested.clone(),
        requested,
        affected,
        leaves,
        gaps,
        explanations,
    })
}

/// Orders included leaves by invalidation mass, then conclusion id.
///
/// Learned cost, critical-path gain, and novelty gain are diagnostic and this
/// subject records none of them, so they add no term and do not change order.
pub(crate) fn legal_order(left: &Leaf, right: &Leaf) -> std::cmp::Ordering {
    right
        .facts
        .len()
        .cmp(&left.facts.len())
        .then_with(|| left.conclusion.cmp(&right.conclusion))
}

fn classify(spec: &Spec) -> Result<&'static str, WorldError> {
    if spec.owner.is_none() {
        return Ok("unknown-owner");
    }
    for fact in spec.facts {
        match body_of(fact)? {
            Some(_) => {}
            None => return Ok("missing-source"),
        }
    }
    if spec.status == Some("revoked") {
        return Ok("revoked-evidence");
    }
    Ok("legal-ready")
}

fn body_of(fact: &str) -> Result<Option<&str>, WorldError> {
    BODIES
        .iter()
        .find(|(id, _)| *id == fact)
        .map(|(_, body)| *body)
        .ok_or_else(|| WorldError::Qualification(format!("{fact} is outside the sealed universe")))
}

fn subject_datum() -> Datum {
    let facts = BODIES
        .iter()
        .map(|(id, body)| {
            let mut fields = vec![(Symbol::new("id"), Datum::String((*id).to_owned()))];
            if let Some(body) = body {
                fields.push((Symbol::new("body"), Datum::String((*body).to_owned())));
            }
            Datum::Node {
                tag: Symbol::qualified("world", "plan-fact-v1"),
                fields,
            }
        })
        .collect();
    let conclusions = SPECS
        .iter()
        .map(|spec| {
            let mut fields = vec![
                (Symbol::new("id"), Datum::String(spec.conclusion.to_owned())),
                (
                    Symbol::new("facts"),
                    Datum::Vector(
                        spec.facts
                            .iter()
                            .map(|fact| Datum::String((*fact).to_owned()))
                            .collect(),
                    ),
                ),
            ];
            if let Some(owner) = spec.owner {
                fields.push((Symbol::new("owner"), Datum::String(owner.to_owned())));
            }
            if let Some(evidence) = spec.evidence {
                fields.push((Symbol::new("evidence"), Datum::String(evidence.to_owned())));
            }
            if let Some(status) = spec.status {
                fields.push((Symbol::new("status"), Datum::String(status.to_owned())));
            }
            Datum::Node {
                tag: Symbol::qualified("world", "plan-conclusion-v1"),
                fields,
            }
        })
        .collect();
    Datum::Node {
        tag: Symbol::qualified("world", "plan-subject-v1"),
        fields: vec![
            (Symbol::new("facts"), Datum::Vector(facts)),
            (Symbol::new("conclusions"), Datum::Vector(conclusions)),
        ],
    }
}

fn page_count(explanations: usize) -> usize {
    if explanations == 0 {
        1
    } else {
        explanations.div_ceil(PAGE_ROWS)
    }
}

fn explanation_slice(subject: &Subject, index: usize) -> Result<&[Explained], WorldError> {
    let pages = page_count(subject.explanations.len());
    if index >= pages {
        return Err(WorldError::Qualification(
            "plan page is outside the sealed subject".to_owned(),
        ));
    }
    if subject.explanations.is_empty() {
        return Ok(&[]);
    }
    let start = index * PAGE_ROWS;
    let end = (start + PAGE_ROWS).min(subject.explanations.len());
    let slice = &subject.explanations[start..end];
    for item in slice {
        if item.path.len() > MAX_CLOSURE_DEPTH {
            return Err(WorldError::Qualification(
                "closure depth exceeded the query budget".to_owned(),
            ));
        }
    }
    let encoded = Datum::Vector(
        slice
            .iter()
            .map(explanation_datum)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let bytes = encoded
        .canonical_bytes()
        .map_err(|error| WorldError::Qualification(error.to_string()))?
        .len();
    if bytes > MAX_BYTES {
        return Err(WorldError::Qualification(
            "plan page exceeded the byte budget".to_owned(),
        ));
    }
    Ok(slice)
}

fn continuation_token(subject: &Subject, index: usize) -> Result<String, WorldError> {
    hex_id(Datum::Node {
        tag: Symbol::qualified("world", "plan-continuation-v1"),
        fields: vec![
            (Symbol::new("world"), Datum::String(subject.world.clone())),
            (Symbol::new("page"), query::count_datum(usize_count(index))),
        ],
    })
}

fn page_index_for_token(subject: &Subject, token: &str) -> Result<Option<usize>, WorldError> {
    let pages = page_count(subject.explanations.len());
    for index in 1..pages {
        if continuation_token(subject, index)? == token {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn project(view: View, subject: &Subject, index: usize) -> Result<Datum, WorldError> {
    let slice = explanation_slice(subject, index)?;
    let continuation = if index + 1 < page_count(subject.explanations.len()) {
        Some(continuation_token(subject, index + 1)?)
    } else {
        None
    };
    Ok(match view {
        View::Expression => expression(subject, index, slice, continuation.as_deref())?,
        View::Text => text(subject, index, slice, continuation.as_deref()),
        View::Card => card(subject, continuation.as_deref()),
    })
}

fn expression(
    subject: &Subject,
    index: usize,
    slice: &[Explained],
    continuation: Option<&str>,
) -> Result<Datum, WorldError> {
    let explanations = slice
        .iter()
        .map(explanation_datum)
        .collect::<Result<Vec<_>, _>>()?;
    let closure = slice.iter().map(edge_datum).collect();
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "proof-plan-v1"),
        fields: vec![
            (Symbol::new("world"), Datum::String(subject.world.clone())),
            (Symbol::new("page"), query::count_datum(usize_count(index))),
            (
                Symbol::new("pages"),
                query::count_datum(usize_count(page_count(subject.explanations.len()))),
            ),
            (Symbol::new("requested"), strings(&subject.requested)),
            (Symbol::new("affected"), strings(&subject.affected)),
            (Symbol::new("legal-ready"), strings(&subject.legal_ready)),
            (Symbol::new("candidates"), strings(&subject.legal_ready)),
            (Symbol::new("reused"), Datum::Vector(Vec::new())),
            (
                Symbol::new("leaves"),
                Datum::Vector(subject.leaves.iter().map(leaf_datum).collect()),
            ),
            (
                Symbol::new("gaps"),
                Datum::Vector(subject.gaps.iter().map(gap_datum).collect()),
            ),
            (Symbol::new("floor"), strings(&subject.floor)),
            (Symbol::new("closure"), Datum::Vector(closure)),
            (Symbol::new("explanations"), Datum::Vector(explanations)),
            (
                Symbol::new("continuation"),
                Datum::String(continuation.unwrap_or("").to_owned()),
            ),
            (Symbol::new("budget"), budget_datum()),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("resident"), Datum::Bool(true)),
        ],
    })
}

fn text(subject: &Subject, index: usize, slice: &[Explained], continuation: Option<&str>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "proof-plan-text-v1"),
        fields: vec![
            (Symbol::new("world"), Datum::String(subject.world.clone())),
            (
                Symbol::new("affected"),
                Datum::String(subject.affected.join(" ")),
            ),
            (
                Symbol::new("legal-ready"),
                Datum::String(subject.legal_ready.join(" ")),
            ),
            (Symbol::new("floor"), Datum::String(subject.floor.join(" "))),
            (Symbol::new("gaps"), Datum::String(gap_text(&subject.gaps))),
            (
                Symbol::new("leaves"),
                Datum::String(leaf_text(&subject.leaves)),
            ),
            (
                Symbol::new("explanations"),
                Datum::String(explanation_text(slice)),
            ),
            (
                Symbol::new("continuation"),
                Datum::String(continuation.unwrap_or("").to_owned()),
            ),
            (Symbol::new("page"), query::count_datum(usize_count(index))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("resident"), Datum::Bool(true)),
        ],
    }
}

/// Projects the fixed card predicates. `requires` carries the typed needs
/// and `args` carries the continuation of this sealed subject.
fn card(subject: &Subject, continuation: Option<&str>) -> Datum {
    let values = [
        Datum::String(subject.world.clone()),
        Datum::String(PLAN_KIND.to_owned()),
        Datum::String("Deterministic proof plan for one sealed subject.".to_owned()),
        Datum::String(continuation.unwrap_or("").to_owned()),
        Datum::String(subject.legal_ready.join(" ")),
        Datum::String(String::new()),
        Datum::String("plan".to_owned()),
        Datum::String(gap_text(&subject.gaps)),
        Datum::String("why".to_owned()),
        Datum::Bool(true),
    ];
    let predicates = sim_kernel::card::card_fixed_predicates();
    debug_assert_eq!(predicates.len(), values.len());
    Datum::Node {
        tag: Symbol::qualified("world", "proof-plan-card-v1"),
        fields: predicates.into_iter().zip(values).collect(),
    }
}

fn stale_gap(subject: &Subject) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "plan-gap-v1"),
        fields: vec![
            (
                Symbol::new("reason"),
                Datum::String("stale-or-missing".to_owned()),
            ),
            (Symbol::new("world"), Datum::String(subject.world.clone())),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("resident"), Datum::Bool(true)),
        ],
    }
}

fn budget_datum() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "query-budget-v1"),
        fields: vec![
            (
                Symbol::new("bytes"),
                query::count_datum(usize_count(MAX_BYTES)),
            ),
            (
                Symbol::new("rows"),
                query::count_datum(usize_count(PAGE_ROWS)),
            ),
            (
                Symbol::new("closure-depth"),
                query::count_datum(usize_count(MAX_CLOSURE_DEPTH)),
            ),
            (
                Symbol::new("work"),
                query::count_datum(usize_count(MAX_WORK)),
            ),
        ],
    }
}

fn leaf_datum(leaf: &Leaf) -> Datum {
    let mut fields = vec![
        (
            Symbol::new("conclusion"),
            Datum::String(leaf.conclusion.clone()),
        ),
        (Symbol::new("included"), Datum::Bool(leaf.included)),
        (Symbol::new("reason"), Datum::String(leaf.reason.to_owned())),
        (Symbol::new("facts"), strings(&leaf.facts)),
    ];
    if let Some(evidence) = &leaf.evidence {
        fields.push((Symbol::new("evidence"), Datum::String(evidence.clone())));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "plan-leaf-v1"),
        fields,
    }
}

fn gap_datum((conclusion, reason): &(String, &'static str)) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "typed-need-v1"),
        fields: vec![
            (Symbol::new("conclusion"), Datum::String(conclusion.clone())),
            (Symbol::new("reason"), Datum::String((*reason).to_owned())),
        ],
    }
}

fn edge_datum(item: &Explained) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "closure-edge-v1"),
        fields: vec![
            (
                Symbol::new("conclusion"),
                Datum::String(item.conclusion.clone()),
            ),
            (Symbol::new("fact"), Datum::String(item.fact.clone())),
            (Symbol::new("path"), strings(&item.path)),
        ],
    }
}

fn explanation_datum(item: &Explained) -> Result<Datum, WorldError> {
    Ok(explanation_value(Explanation {
        conclusion: ConclusionId::new(item.conclusion.clone())?,
        fact: FactId::new(item.fact.clone())?,
        path: item.path.clone(),
    }))
}

fn strings(values: &[String]) -> Datum {
    Datum::Vector(values.iter().cloned().map(Datum::String).collect())
}

fn gap_text(gaps: &[(String, &'static str)]) -> String {
    gaps.iter()
        .map(|(conclusion, reason)| format!("{conclusion} {reason}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn leaf_text(leaves: &[Leaf]) -> String {
    leaves
        .iter()
        .map(|leaf| {
            let disposition = if leaf.included {
                "included"
            } else {
                "excluded"
            };
            format!("{} {disposition} {}", leaf.conclusion, leaf.reason)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn explanation_text(items: &[Explained]) -> String {
    items
        .iter()
        .map(|item| format!("{} {}", item.conclusion, item.fact))
        .collect::<Vec<_>>()
        .join("; ")
}

fn hex_id(value: Datum) -> Result<String, WorldError> {
    let id = content_id(value)?;
    Ok(hex(&id.bytes))
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn usize_count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
