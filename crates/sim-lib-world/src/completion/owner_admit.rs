// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner receipts that may enter the final-world catalog.
//!
//! The evidence owner is the only production admission path. Its checked
//! subject and result must be this catalog's subject and the claim result.
//! A source-deck label is not read. The lantern fixture is not copied onto
//! another subject. This module does not derive a seal.

use std::collections::BTreeMap;

use sim_lib_roadmap_runner::AdmittedEvidence;

use super::{
    CheckerCatalog, ClaimAdmission, OwnerCheckerReceiptId, ReleaseObligation, WorldSealId,
};
use crate::WorldError;

/// Admissions whose owner receipt matches `subject`, and the catalog that checked them.
///
/// # Errors
///
/// Returns an error when the evidence owner cannot build its receipt.
pub(crate) fn from_owner(
    subject: &str,
    releases: &[ReleaseObligation],
) -> Result<(Vec<ClaimAdmission>, Option<CheckerCatalog>), WorldError> {
    let release = crate::evidence::owner_release()
        .map_err(|error| WorldError::Qualification(format!("evidence owner admission: {error}")))?;
    if release.subject != subject {
        return Ok((Vec::new(), None));
    }
    let Some(claim) = claim_from_result(&release.result) else {
        return Ok((Vec::new(), None));
    };
    let checks = CheckerCatalog {
        catalog: release.catalog.clone(),
        scope: release.scope.clone(),
        revocations: BTreeMap::from([(release.checker.clone(), release.revocation.clone())]),
    };
    let row = CheckedAdmit {
        claim,
        admitted: release.admitted.clone(),
        checker: release.checker.clone(),
    };
    let admitted = admit_checked(subject, releases, &[row], &checks);
    Ok((admitted, Some(checks)))
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
mod tests {
    use super::super::CheckerCatalog;
    use super::claim_from_result;

    #[test]
    fn a_source_deck_label_is_not_a_claim() {
        for label in [
            "pass phase/28",
            "fresh specimen construction-specimen",
            "evidence same",
            "subject/lantern",
            "result/published",
            "completion/claim-1 extra",
            "completion/claim-01",
            "completion/claim-21 ",
            "completion/claim-0",
            "completion/claim-26",
        ] {
            assert!(claim_from_result(label).is_none(), "{label}");
        }
        assert_eq!(claim_from_result("completion/claim-1"), Some(1));
        assert_eq!(claim_from_result("completion/claim-21"), Some(21));
        assert_eq!(
            claim_from_result(
                "completion/claim-21 git+0123456789abcdef0123456789abcdef01234567:sim-example@0.1.0"
            ),
            Some(21)
        );
    }

    #[test]
    fn the_lantern_owner_is_not_admitted_for_any_subject() {
        let (lantern, lantern_checks) = super::from_owner("subject/lantern", &[]).expect("owner");
        assert!(lantern.is_empty());
        assert!(lantern_checks.is_none());
        let (live, live_checks) = super::from_owner(&"ab".repeat(32), &[]).expect("owner");
        assert!(live.is_empty());
        assert!(live_checks.is_none());
    }

    #[test]
    fn a_verified_receipt_binds_only_its_own_subject() {
        let scoped = crate::evidence::scoped_release("world-final", "scope/completion")
            .expect("scoped fixture");
        let checks = CheckerCatalog {
            catalog: scoped.catalog,
            scope: scoped.scope,
            revocations: scoped.revocations,
        };
        let rows: Vec<_> = scoped
            .admissions
            .into_iter()
            .map(|(claim, admitted, checker)| super::CheckedAdmit {
                claim,
                admitted,
                checker,
            })
            .collect();
        let bound = super::admit_checked("world-final", &[], &rows, &checks);
        assert_eq!(bound.len(), 25);
        assert!(bound.iter().any(|row| row.claim == 1));
        let other = super::admit_checked("subject/lantern", &[], &rows, &checks);
        assert!(other.is_empty());
    }

    #[test]
    fn a_relabelled_receipt_stays_out() {
        let scoped = crate::evidence::scoped_release_results(
            "world-final",
            "scope/completion",
            &[(1, "result/published")],
        )
        .expect("scoped fixture");
        let checks = CheckerCatalog {
            catalog: scoped.catalog,
            scope: scoped.scope,
            revocations: scoped.revocations,
        };
        let rows: Vec<_> = scoped
            .admissions
            .into_iter()
            .map(|(claim, admitted, checker)| super::CheckedAdmit {
                claim,
                admitted,
                checker,
            })
            .collect();
        let bound = super::admit_checked("world-final", &[], &rows, &checks);
        assert_eq!(bound.len(), 24);
        assert!(bound.iter().all(|row| row.claim != 1));
    }
}
