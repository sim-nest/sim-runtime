// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner receipts that may enter the final-world catalog.
//!
//! The caller supplies owner receipts the checker already admitted. Each
//! claim keeps that owner's revocation map, including a different checker
//! that supported the evidence. The evidence owner is used only when no
//! receipt was supplied. Its checked subject and result must be this
//! catalog's subject and the claim result.
//! A verified source-identity catalog stays at scope `release/source-identity`
//! and does not become a claim. A source-deck label is not read. The lantern
//! fixture is not copied onto another subject. This module does not derive a seal.

use std::cell::RefCell;
use std::collections::BTreeMap;

use sim_conformance_evidence::{CheckScopeId, RevocationStatus};
use sim_lib_roadmap_runner::{AdmittedEvidence, ProofCatalog};

use super::{
    CheckerCatalog, ClaimAdmission, OwnerCheckerReceiptId, ReleaseObligation, WorldSealId,
};
use crate::WorldError;

const SOURCE_IDENTITY_SCOPE: &str = "release/source-identity";
const REPLAY_SCOPE: &str = "completion/replay";
const REPLAY_RESULT: &str = "completion/claim-17";

struct RetainedSourceIdentity {
    subject: String,
    checks: CheckerCatalog,
}

#[derive(Clone)]
struct RetainedReplay {
    subject: String,
    admitted: AdmittedEvidence,
    checker: OwnerCheckerReceiptId,
    checks: CheckerCatalog,
}

#[derive(Clone)]
struct RetainedOwnerClaim {
    subject: String,
    claim: u8,
    admitted: AdmittedEvidence,
    checker: OwnerCheckerReceiptId,
    catalog: ProofCatalog,
    scope: CheckScopeId,
    revocations: BTreeMap<OwnerCheckerReceiptId, RevocationStatus>,
}

/// One owner claim a caller already had the checker admit.
pub struct SuppliedOwnerClaim {
    /// Subject the receipt names.
    pub subject: String,
    /// Claim number, from 1 through 25.
    pub claim: u8,
    /// Evidence the catalog admitted.
    pub admitted: AdmittedEvidence,
    /// Receipt that bound the admission.
    pub checker: OwnerCheckerReceiptId,
    /// Catalog that admitted the evidence.
    pub catalog: ProofCatalog,
    /// Scope recorded on that receipt.
    pub scope: CheckScopeId,
    /// Revocation state of the claim checker and every checker it consulted.
    pub revocations: BTreeMap<OwnerCheckerReceiptId, RevocationStatus>,
}

thread_local! {
    static RETAINED_SOURCE_IDENTITY: RefCell<Option<RetainedSourceIdentity>> =
        const { RefCell::new(None) };
    static RETAINED_REPLAY: RefCell<Option<RetainedReplay>> = const { RefCell::new(None) };
    static RETAINED_OWNER_CLAIMS: RefCell<Vec<RetainedOwnerClaim>> =
        const { RefCell::new(Vec::new()) };
    static SUPPLIED_OWNER_CLAIMS: RefCell<Vec<SuppliedOwnerClaim>> =
        const { RefCell::new(Vec::new()) };
    static SUPPLIED_REPLAY: RefCell<Option<SuppliedReplay>> = const { RefCell::new(None) };
    static SUPPLIED_SOURCE: RefCell<Option<SuppliedSource>> = const { RefCell::new(None) };
}

/// One source-identity catalog a caller already had the checker admit.
pub(crate) struct SuppliedSource {
    /// Subject the receipt names.
    pub subject: String,
    /// Checker result. A completion claim is refused.
    pub result: String,
    /// Catalog that admitted the evidence.
    pub catalog: ProofCatalog,
    /// Receipt that bound the admission.
    pub checker: OwnerCheckerReceiptId,
    /// Scope recorded on that receipt.
    pub scope: CheckScopeId,
    /// Revocation state recorded for that checker.
    pub revocation: RevocationStatus,
}

/// One journal catalog a caller already had the checker admit.
///
/// This module retains the catalog. It does not run the checker.
pub(crate) struct SuppliedReplay {
    /// Subject the receipt names.
    pub subject: String,
    /// Checker result. Claim 17 requires `completion/claim-17`.
    pub result: String,
    /// Catalog that admitted the evidence.
    pub catalog: ProofCatalog,
    /// Evidence the catalog admitted.
    pub admitted: AdmittedEvidence,
    /// Receipt that bound the admission.
    pub checker: OwnerCheckerReceiptId,
    /// Scope recorded on that receipt.
    pub scope: CheckScopeId,
    /// Revocation state recorded for that checker.
    pub revocation: RevocationStatus,
}

/// Holds one journal catalog until the next final-catalog assembly takes it.
#[cfg(test)]
pub(crate) fn supply_journal_observation(observation: SuppliedReplay) {
    SUPPLIED_REPLAY.with(|slot| *slot.borrow_mut() = Some(observation));
}

pub(crate) fn take_supplied_journal() -> Option<SuppliedReplay> {
    SUPPLIED_REPLAY.with(|slot| slot.borrow_mut().take())
}

pub(crate) fn take_supplied_source() -> Option<SuppliedSource> {
    SUPPLIED_SOURCE.with(|slot| slot.borrow_mut().take())
}

/// Retains one verified source-identity catalog for `subject`.
///
/// The catalog stays at scope `release/source-identity`. A completion-claim
/// result is refused and leaves the previous catalog untouched.
///
/// # Errors
///
/// Returns an error when the result is not source-identity evidence or the
/// scope is not `release/source-identity`.
pub(crate) fn retain_source_identity(
    subject: &str,
    result: &str,
    catalog: ProofCatalog,
    checker: OwnerCheckerReceiptId,
    scope: CheckScopeId,
    revocation: RevocationStatus,
) -> Result<(), WorldError> {
    if !is_source_identity(result, &scope) {
        return Err(WorldError::Qualification(
            "source-identity catalog was refused".to_owned(),
        ));
    }
    let checks = CheckerCatalog {
        catalog,
        scope,
        revocations: BTreeMap::from([(checker, revocation)]),
        owners: BTreeMap::new(),
    };
    RETAINED_SOURCE_IDENTITY.with(|slot| {
        *slot.borrow_mut() = Some(RetainedSourceIdentity {
            subject: subject.to_owned(),
            checks,
        });
    });
    Ok(())
}

fn is_source_identity(result: &str, scope: &CheckScopeId) -> bool {
    let Ok(expected) = CheckScopeId::from_text(SOURCE_IDENTITY_SCOPE) else {
        return false;
    };
    scope == &expected
        && result.starts_with("source-identity ")
        && !result.contains("completion/claim-")
        && claim_from_result(result).is_none()
}

fn retained_source_identity(subject: &str) -> Option<CheckerCatalog> {
    RETAINED_SOURCE_IDENTITY.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|retained| (retained.subject == subject).then(|| retained.checks.clone()))
    })
}

pub(crate) fn clear_source_identity() {
    RETAINED_SOURCE_IDENTITY.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn clear_journal_completion() {
    RETAINED_REPLAY.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn clear_owner_claims() {
    RETAINED_OWNER_CLAIMS.with(|slot| slot.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn clear_supplied_owner_claims() {
    SUPPLIED_OWNER_CLAIMS.with(|slot| slot.borrow_mut().clear());
}

/// Holds one owner claim until the next final-catalog assembly takes it.
///
/// The prove catalog accepts matching claims on the calling thread. This
/// does not run a checker and does not derive a seal.
pub fn supply_owner_claim(claim: SuppliedOwnerClaim) {
    SUPPLIED_OWNER_CLAIMS.with(|slot| slot.borrow_mut().push(claim));
}

/// Retains supplied owner claims that name `subject`. Other subjects are dropped.
pub(crate) fn accept_supplied_owner_claims(subject: &str) {
    for claim in take_supplied_owner_claims() {
        if claim.subject != subject {
            continue;
        }
        retain_owner_claim(
            &claim.subject,
            claim.claim,
            claim.admitted,
            claim.checker,
            claim.catalog,
            claim.scope,
            claim.revocations,
        );
    }
}

fn take_supplied_owner_claims() -> Vec<SuppliedOwnerClaim> {
    SUPPLIED_OWNER_CLAIMS.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

/// Retains one owner-backed claim for `subject`.
///
/// Claim 17 stays on the journal path when that export is retained. A claim
/// outside 1..=25 is ignored. A map that does not name `checker` is ignored.
/// The map keeps every consulted checker, including support checked by
/// another owner. The catalog is the proof for `checker` only.
pub(crate) fn retain_owner_claim(
    subject: &str,
    claim: u8,
    admitted: AdmittedEvidence,
    checker: OwnerCheckerReceiptId,
    catalog: ProofCatalog,
    scope: CheckScopeId,
    revocations: BTreeMap<OwnerCheckerReceiptId, RevocationStatus>,
) {
    if !(1..=25).contains(&claim) || !revocations.contains_key(&checker) {
        return;
    }
    RETAINED_OWNER_CLAIMS.with(|slot| {
        slot.borrow_mut().push(RetainedOwnerClaim {
            subject: subject.to_owned(),
            claim,
            admitted,
            checker,
            catalog,
            scope,
            revocations,
        });
    });
}

fn retained_owner_claims(subject: &str) -> Vec<RetainedOwnerClaim> {
    RETAINED_OWNER_CLAIMS.with(|slot| {
        slot.borrow()
            .iter()
            .filter(|row| row.subject == subject)
            .cloned()
            .collect()
    })
}

/// Retains one journal catalog the caller already had the checker admit.
///
/// A result other than `completion/claim-17`, a different scope, or a
/// revocation other than current retains nothing and does not fail the
/// catalog. This function does not inspect an export body and does not run
/// a conformance pack.
///
/// # Errors
///
/// Returns an error when the replay scope text cannot be constructed.
pub(crate) fn retain_journal_completion(
    subject: &str,
    result: &str,
    catalog: ProofCatalog,
    admitted: AdmittedEvidence,
    checker: OwnerCheckerReceiptId,
    scope: CheckScopeId,
    revocation: RevocationStatus,
) -> Result<(), WorldError> {
    let expected = CheckScopeId::from_text(REPLAY_SCOPE).map_err(|_| {
        WorldError::Qualification("journal completion catalog was refused".to_owned())
    })?;
    if result != REPLAY_RESULT || scope != expected || revocation != RevocationStatus::Current {
        return Ok(());
    }
    let checks = CheckerCatalog {
        catalog,
        scope,
        revocations: BTreeMap::from([(checker.clone(), revocation)]),
        owners: BTreeMap::new(),
    };
    RETAINED_REPLAY.with(|slot| {
        *slot.borrow_mut() = Some(RetainedReplay {
            subject: subject.to_owned(),
            admitted,
            checker,
            checks,
        });
    });
    Ok(())
}

fn retained_replay(subject: &str) -> Option<RetainedReplay> {
    RETAINED_REPLAY.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|retained| (retained.subject == subject).then(|| retained.clone()))
    })
}

/// Admissions whose owner receipt matches `subject`, and the catalog that checked them.
///
/// A retained journal export admits claim 17 when its receipt verifies.
/// Other retained owner claims for that subject are admitted beside it.
/// Each is checked in its own catalog. A claim number already queued is
/// skipped. A source-identity catalog adds no claim. The lantern fixture
/// supplies a claim only when nothing retained matches and its result is
/// that claim.
///
/// # Errors
///
/// Returns an error when the evidence owner cannot build its receipt.
pub(crate) fn from_owner(
    subject: &str,
    releases: &[ReleaseObligation],
) -> Result<(Vec<ClaimAdmission>, Option<CheckerCatalog>), WorldError> {
    let mut queued = Vec::new();
    let checks = retained_replay(subject).map(|replay| {
        queued.push(CheckedAdmit {
            claim: 17,
            admitted: replay.admitted,
            checker: replay.checker,
        });
        replay.checks
    });
    let mut extras = retained_owner_claims(subject);
    extras.retain(|extra| {
        if queued.iter().any(|row| row.claim == extra.claim) {
            return false;
        }
        queued.push(CheckedAdmit {
            claim: extra.claim,
            admitted: extra.admitted.clone(),
            checker: extra.checker.clone(),
        });
        true
    });
    if queued.is_empty() {
        return unmatched_owner(subject, releases);
    }
    let mut checks = match checks {
        Some(checks) => checks,
        None => catalog_from_owner(extras.remove(0)),
    };
    for extra in extras {
        merge_revocations(&mut checks, extra.revocations);
        checks.owners.insert(extra.checker, extra.catalog);
    }
    queued.sort_by_key(|row| row.claim);
    Ok((
        admit_checked(subject, releases, &queued, &checks),
        Some(checks),
    ))
}

fn catalog_from_owner(row: RetainedOwnerClaim) -> CheckerCatalog {
    CheckerCatalog {
        catalog: row.catalog,
        scope: row.scope,
        revocations: row.revocations,
        owners: BTreeMap::new(),
    }
}

fn merge_revocations(
    checks: &mut CheckerCatalog,
    revocations: BTreeMap<OwnerCheckerReceiptId, RevocationStatus>,
) {
    for (id, status) in revocations {
        checks.revocations.entry(id).or_insert(status);
    }
}

fn unmatched_owner(
    subject: &str,
    releases: &[ReleaseObligation],
) -> Result<(Vec<ClaimAdmission>, Option<CheckerCatalog>), WorldError> {
    if let Some(checks) = retained_source_identity(subject) {
        return Ok((Vec::new(), Some(checks)));
    }
    let release = crate::evidence::owner_release()
        .map_err(|error| WorldError::Qualification(format!("evidence owner admission: {error}")))?;
    if release.subject != subject {
        return Ok((Vec::new(), None));
    }
    let Some(claim) = claim_from_result(&release.result) else {
        return Ok((Vec::new(), None));
    };
    let checks = catalog_from_owner(RetainedOwnerClaim {
        subject: subject.to_owned(),
        claim,
        admitted: release.admitted.clone(),
        checker: release.checker.clone(),
        catalog: release.catalog.clone(),
        scope: release.scope.clone(),
        revocations: BTreeMap::from([(release.checker.clone(), release.revocation.clone())]),
    });
    let row = CheckedAdmit {
        claim,
        admitted: release.admitted.clone(),
        checker: release.checker.clone(),
    };
    Ok((
        admit_checked(subject, releases, &[row], &checks),
        Some(checks),
    ))
}

struct CheckedAdmit {
    claim: u8,
    admitted: AdmittedEvidence,
    checker: OwnerCheckerReceiptId,
}

fn admit_checked(
    subject: &str,
    releases: &[ReleaseObligation],
    rows: &[CheckedAdmit],
    checks: &CheckerCatalog,
) -> Vec<ClaimAdmission> {
    let world = WorldSealId {
        value: subject.to_owned(),
    };
    let mut admitted = Vec::new();
    for row in rows {
        if !(1..=25).contains(&row.claim) {
            continue;
        }
        let expected = super::check::claim_result(row.claim, releases);
        let claim = ClaimAdmission {
            claim: row.claim,
            admitted: row.admitted.clone(),
            checker: row.checker.clone(),
        };
        if super::check::release_receipt(checks, &world, &claim, &expected).is_ok() {
            admitted.push(claim);
        }
    }
    admitted
}

fn claim_from_result(result: &str) -> Option<u8> {
    let (head, tail) = match result.split_once(' ') {
        Some((head, tail)) => {
            if tail.is_empty() || tail.split(' ').any(str::is_empty) {
                return None;
            }
            (head, true)
        }
        None => (result, false),
    };
    let number = head.strip_prefix("completion/claim-")?;
    let claim = number.parse::<u8>().ok()?;
    if !(1..=25).contains(&claim) || format!("completion/claim-{claim}") != head {
        return None;
    }
    if tail && claim != 21 {
        return None;
    }
    Some(claim)
}

#[cfg(test)]
#[path = "owner_admit_tests.rs"]
mod tests;
