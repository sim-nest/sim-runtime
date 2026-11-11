// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Input closure an artifact-equality leaf submits to the published catalog.
//!
//! The catalog recomputes this closure and admits the claim only when the
//! retained checker invocation carries the same closure. The catalog stores
//! the receipt and decides reuse.

use sha2::{Digest, Sha256};
use sim_conformance_evidence::CheckInputClosureId;
use sim_kernel::ContentId;
use sim_lib_roadmap_runner::{DiscoveredDefinition, EvidenceClaim, ProofError, ProofLeaf};

pub(super) fn input_closure(
    leaf: &ProofLeaf,
    claim: &EvidenceClaim,
    selected: &[&DiscoveredDefinition],
) -> Result<CheckInputClosureId, ProofError> {
    let definition = proof_definition(leaf)?;
    let code = proof_code(leaf)?;
    let projections = digest(&sorted_join(&claim.projections));
    let environment = digest(claim.environment.as_str());
    let universe = digest(&universe_text(selected));
    let text = canon(&[
        ("subject", &hex(claim.subject.content_id())),
        ("scope", &hex(claim.scope.content_id())),
        ("proof", &digest(&definition)),
        ("proof-code", &digest(&code)),
        ("projections", &projections),
        ("environment", &environment),
        ("universe", &universe),
    ]);
    CheckInputClosureId::from_text(&text).map_err(|error| ProofError::Invalid(error.to_string()))
}

fn proof_definition(leaf: &ProofLeaf) -> Result<String, ProofError> {
    let ProofLeaf::ArtifactEquality { name, .. } = leaf else {
        return Err(ProofError::Invalid(
            "evidence leaf is not an artifact comparison".into(),
        ));
    };
    Ok(canon(&[
        ("kind", "artifact-equality"),
        ("name", name.as_str()),
    ]))
}

fn proof_code(leaf: &ProofLeaf) -> Result<String, ProofError> {
    let ProofLeaf::ArtifactEquality { name, left, right } = leaf else {
        return Err(ProofError::Invalid(
            "evidence leaf is not an artifact comparison".into(),
        ));
    };
    let left = hex_bytes(left);
    let right = hex_bytes(right);
    Ok(canon(&[
        ("name", name.as_str()),
        ("left", &left),
        ("right", &right),
    ]))
}

fn universe_text(selected: &[&DiscoveredDefinition]) -> String {
    let mut rows = selected.to_vec();
    rows.sort_by(|left, right| left.semantic.cmp(&right.semantic));
    rows.dedup_by(|left, right| left.semantic == right.semantic);
    let rendered = rows
        .iter()
        .map(|row| {
            canon(&[
                ("semantic", row.semantic.as_str()),
                ("storage", row.storage.as_str()),
            ])
        })
        .collect::<Vec<_>>();
    length_prefixed(&rendered)
}

fn sorted_join(values: &[String]) -> String {
    let mut values = values.to_vec();
    values.sort();
    length_prefixed(&values)
}

fn length_prefixed(values: &[String]) -> String {
    let mut out = String::new();
    for value in values {
        out.push_str(&value.len().to_string());
        out.push(':');
        out.push_str(value);
        out.push(';');
    }
    out
}

fn canon(fields: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (name, value) in fields {
        out.push_str(name);
        out.push('=');
        out.push_str(&value.len().to_string());
        out.push(':');
        out.push_str(value);
        out.push(';');
    }
    out
}

fn hex(id: &ContentId) -> String {
    hex_bytes(&id.bytes)
}

fn digest(text: &str) -> String {
    hex_bytes(Sha256::digest(text.as_bytes()).as_slice())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
