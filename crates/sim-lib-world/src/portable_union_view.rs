// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Revision-fenced text of one portable phase-union run.
//!
//! Opening the body reruns each grounded leaf and refuses a body whose
//! verdicts, digests, captures, gaps, or request count disagree. The digest
//! binds the retained deck. The same body keeps the receipt detail, both
//! decks, and both claim sets as hex, so the cold record still holds those
//! bytes. A malformed or resigned body is not a proof.

use sim_kernel::Datum;
#[cfg(test)]
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::provider;

pub(super) fn export(proof: &super::UnionProof) -> Result<String, WorldError> {
    seal(&body_of(proof)?)
}

pub(super) fn open(text: &str) -> Result<Option<super::Opened>, WorldError> {
    let Some(body) = unsigned_body(text) else {
        return Ok(None);
    };
    let Some(stated) = revision_token(text) else {
        return Ok(None);
    };
    if stated != revision_of(&body)? {
        return Ok(None);
    }
    let live = super::execute()?;
    if body != body_of(&live)? {
        return Ok(None);
    }
    let admission_requests = live.requests;
    Ok(Some(super::Opened {
        proof: live,
        admission_requests,
    }))
}

fn body_of(proof: &super::UnionProof) -> Result<String, WorldError> {
    let mut parts = Vec::new();
    parts.push(super::HEADER.to_owned());
    push(&mut parts, "subject", &[&proof.subject])?;
    push(
        &mut parts,
        "denominator",
        &[&proof.leaves.len().to_string()],
    )?;
    push(&mut parts, "executed", &[&proof.executed.len().to_string()])?;
    push(&mut parts, "requests", &[&proof.requests.to_string()])?;
    push(
        &mut parts,
        "owner-commands",
        &[&proof.owner_commands.to_string()],
    )?;
    for gate in super::GATES {
        match gate.ground {
            super::Ground::Gap(reason) => push(&mut parts, "gap", &[gate.leaf, reason])?,
            super::Ground::Assay
            | super::Ground::Project
            | super::Ground::Plan
            | super::Ground::Observe
            | super::Ground::Service
            | super::Ground::Release => {
                let Some(digest) = proof.digests.get(gate.leaf) else {
                    return Err(WorldError::Qualification(format!(
                        "proof view leaf {} has no digest",
                        gate.leaf
                    )));
                };
                let Some(retained) = proof.retained.get(gate.leaf) else {
                    return Err(WorldError::Qualification(format!(
                        "proof view leaf {} has no capture",
                        gate.leaf
                    )));
                };
                let mark = if proof.failures.iter().any(|leaf| leaf == gate.leaf) {
                    "fail"
                } else {
                    "pass"
                };
                let detail = hex_field(&retained.detail);
                let actual = hex_field(&retained.actual);
                let expected = hex_field(&retained.expected);
                let required = hex_field(&retained.required);
                let present = hex_field(&retained.present);
                push(
                    &mut parts,
                    mark,
                    &[
                        gate.leaf, digest, &detail, &actual, &expected, &required, &present,
                    ],
                )?;
            }
        }
    }
    Ok(parts.join(";"))
}

fn push(parts: &mut Vec<String>, kind: &str, fields: &[&str]) -> Result<(), WorldError> {
    if fields
        .iter()
        .any(|field| field.is_empty() || field.contains([';', ' ', '\n', '\r']))
    {
        return Err(WorldError::Qualification(
            "proof view field is empty".to_owned(),
        ));
    }
    let mut part = kind.to_owned();
    for field in fields {
        if field.is_empty() {
            return Err(WorldError::Qualification(
                "proof view field is empty".to_owned(),
            ));
        }
        part.push(' ');
        part.push_str(field);
    }
    parts.push(part);
    Ok(())
}

fn seal(body: &str) -> Result<String, WorldError> {
    let digest = revision_of(body)?;
    let Some((header, rest)) = body.split_once(';') else {
        return Err(WorldError::Qualification(
            "proof view body has no records".to_owned(),
        ));
    };
    if header != super::HEADER {
        return Err(WorldError::Qualification(
            "proof view body has the wrong header".to_owned(),
        ));
    }
    Ok(format!("{header};revision {digest};{rest}"))
}

fn revision_of(body: &str) -> Result<String, WorldError> {
    Ok(super::super::record::hex(
        &provider::content_id(Datum::String(body.to_owned()))?.bytes,
    ))
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
    if revision.is_none() || parts.first().copied() != Some(super::HEADER) {
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

fn hex_field(text: &str) -> String {
    if text.is_empty() {
        "-".to_owned()
    } else {
        super::super::record::hex(text.as_bytes())
    }
}

#[cfg(test)]
const DECK_DETAIL: &str = "exact deck identity and required claims";

#[cfg(test)]
fn predicate(actual: &str, present: &str) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/07".to_owned(),
        actual_deck: actual.to_owned(),
        expected_deck: "expected".to_owned(),
        required_claims: std::collections::BTreeSet::from(["conclusion/01".to_owned()]),
        present_claims: std::collections::BTreeSet::from([present.to_owned()]),
    }
}

#[test]
fn a_different_deck_changes_the_leaf_digest() {
    let passed =
        super::digest_of(&predicate("expected", "conclusion/01"), true, DECK_DETAIL).unwrap();
    let other_deck =
        super::digest_of(&predicate("other", "conclusion/01"), true, DECK_DETAIL).unwrap();
    let other_claim =
        super::digest_of(&predicate("expected", "conclusion/02"), true, DECK_DETAIL).unwrap();
    let owner_error = super::digest_of(
        &predicate("owner down", "conclusion/01"),
        false,
        DECK_DETAIL,
    )
    .unwrap();
    assert_eq!(passed.len(), 64);
    assert_ne!(passed, other_deck);
    assert_ne!(passed, other_claim);
    assert_ne!(passed, owner_error);
}

#[test]
fn the_view_keeps_the_deck_that_ran() {
    let proof = super::execute().unwrap();
    let view = export(&proof).unwrap();
    let stored = proof.digests.get("phase/07").expect("assay digest");
    let (actual, expected, required, present) = super::assay_decks().unwrap();
    let leaf = ProofLeaf::SourceDeckPredicate {
        name: "phase/07".to_owned(),
        actual_deck: actual.clone(),
        expected_deck: expected.clone(),
        required_claims: required.clone(),
        present_claims: present.clone(),
    };
    assert_eq!(stored, &super::digest_of(&leaf, true, DECK_DETAIL).unwrap());
    assert!(view.contains(&hex_field(&actual)));
    assert!(view.contains(&hex_field(&expected)));
    let required_text = required.into_iter().collect::<Vec<_>>().join("\n");
    let present_text = present.into_iter().collect::<Vec<_>>().join("\n");
    assert!(view.contains(&hex_field(&required_text)));
    assert!(view.contains(&hex_field(&present_text)));
    assert!(view.contains(&hex_field(DECK_DETAIL)));
    assert!(view.contains("pass phase/07 "));
    assert!(view.contains("gap phase/02 same-computation"));
}
