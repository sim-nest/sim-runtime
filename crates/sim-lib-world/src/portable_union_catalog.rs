// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted facts of one portable phase-union run.
//!
//! The fresh row's deck is the specimen the fresh command captured.
//! A refused Linux row is not a selected Linux boundary.

use crate::WorldError;

/// Facts read from one admitted phase union.
pub(crate) struct PhaseUnionAdmission {
    /// Header of the admitted union.
    pub(crate) roadmap: String,
    /// Content id of the union subject.
    pub(crate) subject: String,
    /// Leaf names and digests, in execution order.
    pub(crate) replay_body: String,
    /// Specimen reason the fresh leaf admitted.
    pub(crate) fresh_reason: String,
    /// Whether a leaf admitted a selected Linux boundary.
    pub(crate) linux_selected: bool,
    /// Whether a leaf admitted a model-provider credential.
    pub(crate) credential: bool,
}

/// Reads the admitted phase union.
///
/// # Errors
///
/// Returns an error when a grounded leaf is missing its digest or the
/// fresh leaf did not admit its specimen reason.
pub(crate) fn admitted_phase_union() -> Result<PhaseUnionAdmission, WorldError> {
    let proof = super::execute()?;
    let fresh = fresh_gate()?;
    if !proof.executed.iter().any(|leaf| leaf == fresh) {
        return Err(WorldError::Qualification(
            "fresh leaf was not admitted".to_owned(),
        ));
    }
    let retained = proof
        .retained
        .get(fresh)
        .ok_or_else(|| WorldError::Qualification("fresh leaf has no capture".to_owned()))?;
    let reason = specimen_reason(&retained.actual, &retained.present)?;
    Ok(PhaseUnionAdmission {
        roadmap: super::HEADER.to_owned(),
        subject: proof.subject.clone(),
        replay_body: replay_body(&proof)?,
        fresh_reason: reason,
        linux_selected: selects_linux(&proof),
        credential: holds_credential(&proof),
    })
}

fn fresh_gate() -> Result<&'static str, WorldError> {
    super::GATES
        .iter()
        .find(|gate| matches!(gate.ground, super::Ground::Fresh))
        .map(|gate| gate.leaf)
        .ok_or_else(|| WorldError::Qualification("fresh leaf is not in the phase union".to_owned()))
}

fn replay_body(proof: &super::UnionProof) -> Result<String, WorldError> {
    let mut text = String::new();
    for name in &proof.executed {
        let Some(digest) = proof.digests.get(name) else {
            return Err(WorldError::Qualification(format!(
                "phase union leaf {name} has no digest"
            )));
        };
        text.push_str(name);
        text.push(' ');
        text.push_str(digest);
        text.push('\n');
    }
    if text.is_empty() {
        return Err(WorldError::Qualification(
            "phase union admitted no leaf".to_owned(),
        ));
    }
    Ok(text)
}

fn specimen_reason(actual: &str, present: &str) -> Result<String, WorldError> {
    let Some(reason) = labeled(actual, "specimen") else {
        return Err(WorldError::Qualification(
            "fresh leaf has no specimen".to_owned(),
        ));
    };
    if reason.contains(|ch: char| ch.is_whitespace()) {
        return Err(WorldError::Qualification(
            "fresh specimen reason is not one token".to_owned(),
        ));
    }
    let claim = format!("fresh specimen {reason}");
    if !present.lines().any(|line| line == claim) {
        return Err(WorldError::Qualification(
            "fresh leaf did not admit the specimen reason".to_owned(),
        ));
    }
    Ok(reason.to_owned())
}

fn labeled<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    let prefix = format!("{label} ");
    text.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn selects_linux(proof: &super::UnionProof) -> bool {
    proof.retained.values().any(|row| {
        row.present.lines().any(|line| line == "linux-selected")
            || row
                .actual
                .lines()
                .any(|line| line == "linux selected-unfinished")
    })
}

fn holds_credential(proof: &super::UnionProof) -> bool {
    proof.retained.values().any(|row| {
        row.present
            .lines()
            .any(|line| line == "model-provider-credential")
    })
}
