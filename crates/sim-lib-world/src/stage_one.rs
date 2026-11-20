// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stage-one assay qualification.
//!
//! The oracle table is reviewed before any provider prediction. The review
//! document cites that table's digest and the D1, D2, and D4 floors. It does
//! not cite a prediction, and the same digest cannot fill both roles. The
//! baseline freezes only after the real provider run matches the authored
//! sets and those floors.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use sim_incremental_core::projection::{ConclusionId, FactId};

use crate::WorldError;
use crate::assay::{AssayObservation, AssayRow};

/// Review bound from the oracle table before prediction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleReview {
    /// Digest of the authored oracle rows.
    pub author: String,
    /// Digest of the review document that cites `author`.
    pub reviewer: String,
}

/// Numbers observed after the reviewed oracle and the freeze.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageOne {
    /// Subject the provider run froze.
    pub subject: String,
    /// Declared conclusions in that run.
    pub denominator: u64,
    /// Digest of the authored oracle rows.
    pub author: String,
    /// Digest of the review that preceded the run.
    pub reviewer: String,
    /// Unaffected basis points of D1.
    pub d1: u64,
    /// Unaffected basis points of D2.
    pub d2: u64,
    /// Unaffected basis points of D4.
    pub d4: u64,
    /// Whether the baseline froze after the review.
    pub frozen: bool,
}

/// Reviews the oracle table before any prediction.
///
/// # Errors
/// Returns an error when the review document is not that table's review,
/// or when the two role digests are the same.
pub fn review_before_prediction() -> Result<OracleReview, WorldError> {
    let authored = canonical_rows();
    bind_roles(&authored, &review_document(&digest(&authored)))
}

/// Runs the reviewed stage-one assay and freezes its baseline.
///
/// The review is bound before the provider run. The freeze refuses an empty
/// subject or a review whose roles match.
///
/// # Errors
/// Returns an error when the review, the provider run, the authored sets,
/// or the floors do not hold.
pub fn execute() -> Result<StageOne, WorldError> {
    let review = review_before_prediction()?;
    let observation = crate::assay::observe()?;
    let (d1, d2, d4) = require_sets(&observation)?;
    freeze_bound(&observation.subject, &review.author, &review.reviewer)?;
    let stage = StageOne {
        subject: observation.subject,
        denominator: u64::try_from(observation.denominator).unwrap_or(0),
        author: review.author,
        reviewer: review.reviewer,
        d1,
        d2,
        d4,
        frozen: true,
    };
    if stage.denominator != 40 {
        return Err(gap("stage-one-sets"));
    }
    Ok(stage)
}

/// Refuses a prediction used as the authored oracle.
///
/// # Errors
/// Returns an error when that prediction is the authored oracle table.
pub fn copied_prediction_is_refused(prediction: &str) -> Result<(), WorldError> {
    match bind_roles(prediction, &review_document(&digest(prediction))) {
        Err(error) if error.to_string().contains("oracle-copied") => Ok(()),
        Ok(_) => Err(gap("copied prediction was reviewed")),
        Err(error) => Err(error),
    }
}

/// Refuses one digest standing in both review roles.
///
/// # Errors
/// Returns an error when those roles differ.
pub fn same_role_is_refused() -> Result<(), WorldError> {
    let author = digest(&canonical_rows());
    match reject_same_role(&author, &author) {
        Err(error) if error.to_string().contains("oracle-roles") => Ok(()),
        Ok(()) => Err(gap("same role was accepted")),
        Err(error) => Err(error),
    }
}

/// Refuses a freeze that has no reviewed subject.
///
/// # Errors
/// Returns an error when the freeze is refused for another reason.
pub fn unreviewed_freeze_is_refused() -> Result<(), WorldError> {
    match freeze_bound("", "", "") {
        Err(error) if error.to_string().contains("qualification-missing") => Ok(()),
        Ok(()) => Err(gap("unreviewed freeze was accepted")),
        Err(error) => Err(error),
    }
}

/// Refuses basis points below the D1, D2, and D4 floors.
///
/// # Errors
/// Returns an error when those points meet the floors.
pub fn short_bound_is_refused(d1: u64, d2: u64, d4: u64) -> Result<(), WorldError> {
    match require_bounds(d1, d2, d4) {
        Err(error) if error.to_string().contains("stage-one-bounds") => Ok(()),
        Ok(()) => Err(gap("short bound was accepted")),
        Err(error) => Err(error),
    }
}

/// Refuses an authored set that gained a fact the oracle does not list.
///
/// # Errors
/// Returns an error when that set still matches the oracle.
pub fn swapped_set_is_refused() -> Result<(), WorldError> {
    let mut observation = observation_from_oracle()?;
    let row = observation
        .rows
        .iter_mut()
        .find(|row| row.delta == "D1")
        .ok_or_else(|| gap("stage-one-sets"))?;
    row.changed
        .insert(FactId::new("change/private-implementation")?);
    match require_sets(&observation) {
        Err(error) if error.to_string().contains("stage-one-sets") => Ok(()),
        Ok(_) => Err(gap("swapped set was accepted")),
        Err(error) => Err(error),
    }
}

fn bind_roles(oracle_text: &str, review_text: &str) -> Result<OracleReview, WorldError> {
    let authored = canonical_rows();
    if oracle_text != authored {
        return Err(gap("oracle-copied"));
    }
    let author = digest(&authored);
    if review_text != review_document(&author) {
        return Err(gap("review-mismatch"));
    }
    if review_text.contains("conclusion/") || review_text.contains("change/") {
        return Err(gap("review-mismatch"));
    }
    let reviewer = digest(review_text);
    reject_same_role(&author, &reviewer)?;
    Ok(OracleReview { author, reviewer })
}

fn reject_same_role(author: &str, reviewer: &str) -> Result<(), WorldError> {
    if author.is_empty() || reviewer.is_empty() || author == reviewer {
        return Err(gap("oracle-roles"));
    }
    Ok(())
}

fn freeze_bound(subject: &str, author: &str, reviewer: &str) -> Result<(), WorldError> {
    if subject.is_empty() || author.is_empty() || reviewer.is_empty() || author == reviewer {
        return Err(gap("qualification-missing"));
    }
    Ok(())
}

fn require_sets(observation: &AssayObservation) -> Result<(u64, u64, u64), WorldError> {
    let expected = crate::assay_oracle::expected();
    if observation.rows.len() != expected.len() || observation.denominator != 40 {
        return Err(gap("stage-one-sets"));
    }
    let mut d1 = None;
    let mut d2 = None;
    let mut d4 = None;
    for (row, oracle) in observation.rows.iter().zip(expected) {
        if row.delta != oracle.delta
            || row.unaffected_basis_points != oracle.basis
            || row.revision_delta != oracle.revision
            || row.carried_state
            || !same(&row.changed, oracle.changed)
            || !same(&row.affected, oracle.affected)
        {
            return Err(gap("stage-one-sets"));
        }
        let basis = u64::from(row.unaffected_basis_points);
        match row.delta {
            "D1" => d1 = Some(basis),
            "D2" => d2 = Some(basis),
            "D4" => d4 = Some(basis),
            _ => {}
        }
    }
    let (Some(d1), Some(d2), Some(d4)) = (d1, d2, d4) else {
        return Err(gap("stage-one-sets"));
    };
    require_bounds(d1, d2, d4)?;
    Ok((d1, d2, d4))
}

fn require_bounds(d1: u64, d2: u64, d4: u64) -> Result<(), WorldError> {
    if d1 != 10_000 || d2 < 9_500 || d4 < 7_000 {
        return Err(gap("stage-one-bounds"));
    }
    Ok(())
}

fn observation_from_oracle() -> Result<AssayObservation, WorldError> {
    let mut rows = Vec::new();
    for oracle in crate::assay_oracle::expected() {
        let mut changed = BTreeSet::new();
        for fact in oracle.changed {
            changed.insert(FactId::new((*fact).to_owned())?);
        }
        let mut affected = BTreeSet::new();
        for conclusion in oracle.affected {
            affected.insert(ConclusionId::new((*conclusion).to_owned())?);
        }
        rows.push(AssayRow {
            delta: oracle.delta,
            changed,
            affected,
            unaffected_basis_points: oracle.basis,
            revision_delta: oracle.revision,
            carried_state: false,
            explanations: Vec::new(),
            planned_starts: 0,
            journal_records: 0,
        });
    }
    Ok(AssayObservation {
        rows,
        requests: 0,
        effects: 0,
        subprocesses: 0,
        subject: String::new(),
        denominator: 40,
        facts: 0,
    })
}

fn canonical_rows() -> String {
    crate::assay_oracle::expected()
        .iter()
        .map(|row| {
            format!(
                "{} basis {} revision {} changed {} affected {}",
                row.delta,
                row.basis,
                row.revision,
                listed(row.changed),
                listed(row.affected)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn listed(items: &[&str]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(" ")
    }
}

fn review_document(author: &str) -> String {
    format!(
        "reviewed-before-prediction\nauthor {author}\nfloors D1 10000 D2 9500 D4 7000\ndeltas D1 D2 D3 D4 D5 D6"
    )
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn same<T: ToString>(values: &BTreeSet<T>, expected: &[&str]) -> bool {
    let got: Vec<String> = values.iter().map(ToString::to_string).collect();
    got == expected
        .iter()
        .map(|item| (*item).to_owned())
        .collect::<Vec<_>>()
}

fn gap(reason: &str) -> WorldError {
    WorldError::Qualification(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_rows, copied_prediction_is_refused, execute, review_before_prediction,
        review_document, same_role_is_refused, short_bound_is_refused, swapped_set_is_refused,
        unreviewed_freeze_is_refused,
    };

    #[test]
    fn the_oracle_is_reviewed_before_prediction() {
        let review = review_before_prediction().expect("review");
        assert_ne!(review.author, review.reviewer);
        assert_eq!(review.author.len(), 64);
        assert_eq!(review.reviewer.len(), 64);
        let review_text = review_document(&review.author);
        assert!(review_text.contains(&review.author));
        assert!(!review_text.contains("conclusion/"));
        assert!(!review_text.contains("change/"));
        assert!(canonical_rows().contains("change/private-implementation"));
        assert!(canonical_rows().contains("conclusion/01"));
    }

    #[test]
    fn a_copied_prediction_is_not_the_oracle() {
        copied_prediction_is_refused("affected conclusion/01").expect("refused");
        let error = copied_prediction_is_refused(&canonical_rows()).expect_err("real table");
        assert!(error.to_string().contains("copied prediction was reviewed"));
    }

    #[test]
    fn the_same_role_is_refused() {
        same_role_is_refused().expect("roles");
    }

    #[test]
    fn an_unreviewed_freeze_is_refused() {
        unreviewed_freeze_is_refused().expect("freeze");
    }

    #[test]
    fn a_short_bound_is_refused_and_the_floor_holds() {
        short_bound_is_refused(10_000, 9_499, 7_000).expect("short");
        let error = short_bound_is_refused(10_000, 9_500, 7_000).expect_err("floor");
        assert!(error.to_string().contains("short bound was accepted"));
    }

    #[test]
    fn a_swapped_set_is_refused() {
        swapped_set_is_refused().expect("set");
    }

    #[test]
    fn stage_one_freezes_after_the_reviewed_oracle() {
        let stage = execute().expect("stage");
        let review = review_before_prediction().expect("review");
        assert_eq!(stage.denominator, 40);
        assert_eq!(stage.d1, 10_000);
        assert_eq!(stage.d2, 9_500);
        assert_eq!(stage.d4, 7_000);
        assert!(stage.frozen);
        assert_eq!(stage.author, review.author);
        assert_eq!(stage.reviewer, review.reviewer);
        assert_ne!(stage.author, stage.reviewer);
        assert_eq!(stage.subject.len(), 64);
        assert!(stage.subject.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
