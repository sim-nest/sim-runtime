// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted plan subjects.
//!
//! The reference subject is the sealed in-memory example. A caller document
//! starts with `sim-world-plan-v1` and carries its own facts and conclusions.
//! Owner strings are data. They are not matched here.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::{
    ConclusionId, FactId, FederatedClosure, OwnerProjectionGraph,
};
use sim_kernel::{Datum, Symbol};

use super::{Explained, Leaf, MAX_WORK, Subject, WorldError, hex_id, legal_order};

struct Spec {
    conclusion: String,
    facts: Vec<String>,
    evidence: Option<String>,
    status: Option<String>,
    owner: Option<String>,
}

pub(super) fn fixed() -> Result<Subject, WorldError> {
    seal(reference_bodies(), reference_specs())
}

pub(super) fn from_document(text: &str) -> Result<Subject, WorldError> {
    let mut lines = text.lines();
    if lines.next() != Some("sim-world-plan-v1") {
        return Err(WorldError::Qualification(
            "plan subject is not an admitted document".to_owned(),
        ));
    }
    let mut bodies = Vec::new();
    let mut specs = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(' ');
        match parts.next() {
            Some("fact") => {
                let id = token(&mut parts)?;
                let body = token(&mut parts)?;
                if parts.next().is_some() {
                    return Err(WorldError::Qualification(
                        "plan fact has too many fields".to_owned(),
                    ));
                }
                bodies.push((id, absent(body)));
            }
            Some("spec") => {
                let conclusion = token(&mut parts)?;
                let owner = absent(token(&mut parts)?);
                let evidence = absent(token(&mut parts)?);
                let status = absent(token(&mut parts)?);
                let facts = parts.map(str::to_owned).collect();
                specs.push(Spec {
                    conclusion,
                    facts,
                    evidence,
                    status,
                    owner,
                });
            }
            _ => {
                return Err(WorldError::Qualification(
                    "plan document has an unknown line".to_owned(),
                ));
            }
        }
    }
    seal(bodies, specs)
}

fn token(parts: &mut std::str::Split<'_, char>) -> Result<String, WorldError> {
    parts
        .next()
        .map(str::to_owned)
        .ok_or_else(|| WorldError::Qualification("plan document is missing a field".to_owned()))
}

fn absent(token: String) -> Option<String> {
    (token != "-").then_some(token)
}

fn reference_bodies() -> Vec<(String, Option<String>)> {
    vec![
        ("fact/source-a".to_owned(), Some("alpha".to_owned())),
        ("fact/source-b".to_owned(), Some("beta".to_owned())),
        ("fact/source-c".to_owned(), Some("gamma".to_owned())),
        ("fact/source-gap".to_owned(), None),
    ]
}

fn reference_specs() -> Vec<Spec> {
    vec![
        spec(
            "conclusion/gap-source",
            &["fact/source-gap"],
            None,
            None,
            Some("owner/alpha"),
        ),
        spec(
            "conclusion/ready-a",
            &["fact/source-a"],
            Some("evidence/current"),
            Some("current"),
            Some("owner/alpha"),
        ),
        spec(
            "conclusion/ready-b",
            &["fact/source-b", "fact/source-c"],
            None,
            None,
            Some("owner/alpha"),
        ),
        spec(
            "conclusion/revoked",
            &["fact/source-a"],
            Some("evidence/revoked"),
            Some("revoked"),
            Some("owner/alpha"),
        ),
        spec("conclusion/stranger", &[], None, None, None),
    ]
}

fn spec(
    conclusion: &str,
    facts: &[&str],
    evidence: Option<&str>,
    status: Option<&str>,
    owner: Option<&str>,
) -> Spec {
    Spec {
        conclusion: conclusion.to_owned(),
        facts: facts.iter().map(|fact| (*fact).to_owned()).collect(),
        evidence: evidence.map(str::to_owned),
        status: status.map(str::to_owned),
        owner: owner.map(str::to_owned),
    }
}

fn seal(bodies: Vec<(String, Option<String>)>, specs: Vec<Spec>) -> Result<Subject, WorldError> {
    let mut universe = Vec::with_capacity(bodies.len());
    for (fact, _) in &bodies {
        universe.push(FactId::new(fact)?);
    }
    let mut owners: BTreeMap<String, Vec<(ConclusionId, BTreeSet<FactId>)>> = BTreeMap::new();
    for spec in &specs {
        let Some(owner) = &spec.owner else {
            continue;
        };
        let facts = spec
            .facts
            .iter()
            .map(FactId::new)
            .collect::<Result<BTreeSet<_>, _>>()?;
        owners
            .entry(owner.clone())
            .or_default()
            .push((ConclusionId::new(&spec.conclusion)?, facts));
    }
    let mut graphs = Vec::with_capacity(owners.len());
    for (owner, dependencies) in owners {
        graphs.push(
            OwnerProjectionGraph::new(&owner, dependencies)
                .map_err(|error| WorldError::Qualification(error.to_string()))?,
        );
    }
    let closure = FederatedClosure::seal(universe, graphs)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let mut leaves = Vec::with_capacity(specs.len());
    let mut explanations = Vec::new();
    for spec in &specs {
        let reason = classify(spec, &bodies)?;
        if let Some(owner) = &spec.owner {
            let conclusion = ConclusionId::new(&spec.conclusion)?;
            if closure
                .conclusions()
                .all(|sealed| sealed.as_str() != spec.conclusion)
            {
                return Err(WorldError::Qualification(format!(
                    "{owner} dropped {}",
                    spec.conclusion
                )));
            }
            let mut facts = spec.facts.clone();
            facts.sort_unstable();
            for fact in facts {
                let fact_id = FactId::new(&fact)?;
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
            conclusion: spec.conclusion.clone(),
            facts: spec.facts.clone(),
            evidence: spec.evidence.clone(),
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
        world: hex_id(subject_datum(&bodies, &specs))?,
        records: bodies.len(),
        units: specs.len(),
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

fn classify(spec: &Spec, bodies: &[(String, Option<String>)]) -> Result<&'static str, WorldError> {
    if spec.owner.is_none() {
        return Ok("unknown-owner");
    }
    for fact in &spec.facts {
        match body_of(fact, bodies)? {
            Some(_) => {}
            None => return Ok("missing-source"),
        }
    }
    if spec.status.as_deref() == Some("revoked") {
        return Ok("revoked-evidence");
    }
    Ok("legal-ready")
}

fn body_of<'a>(
    fact: &str,
    bodies: &'a [(String, Option<String>)],
) -> Result<Option<&'a str>, WorldError> {
    bodies
        .iter()
        .find(|(id, _)| id == fact)
        .map(|(_, body)| body.as_deref())
        .ok_or_else(|| WorldError::Qualification(format!("{fact} is outside the sealed universe")))
}

fn subject_datum(bodies: &[(String, Option<String>)], specs: &[Spec]) -> Datum {
    let facts = bodies
        .iter()
        .map(|(id, body)| {
            let mut fields = vec![(Symbol::new("id"), Datum::String(id.clone()))];
            if let Some(body) = body {
                fields.push((Symbol::new("body"), Datum::String(body.clone())));
            }
            Datum::Node {
                tag: Symbol::qualified("world", "plan-fact-v1"),
                fields,
            }
        })
        .collect();
    let conclusions = specs
        .iter()
        .map(|spec| {
            let mut fields = vec![
                (Symbol::new("id"), Datum::String(spec.conclusion.clone())),
                (
                    Symbol::new("facts"),
                    Datum::Vector(
                        spec.facts
                            .iter()
                            .map(|fact| Datum::String(fact.clone()))
                            .collect(),
                    ),
                ),
            ];
            if let Some(owner) = &spec.owner {
                fields.push((Symbol::new("owner"), Datum::String(owner.clone())));
            }
            if let Some(evidence) = &spec.evidence {
                fields.push((Symbol::new("evidence"), Datum::String(evidence.clone())));
            }
            if let Some(status) = &spec.status {
                fields.push((Symbol::new("status"), Datum::String(status.clone())));
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
