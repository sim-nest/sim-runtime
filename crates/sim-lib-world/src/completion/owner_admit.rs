// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner receipts that may enter the final-world catalog.
//!
//! The evidence owner is the only production admission path. Its checked
//! subject and result must be this catalog's subject and the claim result.
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

struct RetainedSourceIdentity {
    subject: String,
    checks: CheckerCatalog,
}

thread_local! {
    static RETAINED_SOURCE_IDENTITY: RefCell<Option<RetainedSourceIdentity>> =
        const { RefCell::new(None) };
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

/// Observes `manifest` for `subject` and retains the verified catalog.
///
/// A pack refusal retains nothing.
///
/// # Errors
///
/// Returns an error when the pack refuses the manifest or the receipt is not
/// source-identity evidence.
pub(crate) fn retain_observed_manifest(subject: &str, manifest: &str) -> Result<(), WorldError> {
    let observed =
        match sim_conformance_packs::packs::release::observe_source_identity(subject, manifest) {
            Ok(observed) => observed,
            Err(error) => {
                return Err(WorldError::Qualification(format!(
                    "source-identity: {}",
                    error.code
                )));
            }
        };
    retain_source_identity(
        &observed.subject,
        &observed.result,
        observed.catalog,
        observed.checker,
        observed.scope,
        observed.revocation,
    )
}

/// Admissions whose owner receipt matches `subject`, and the catalog that checked them.
///
/// A retained source-identity catalog is returned with no claim rows. The
/// lantern fixture supplies a claim only when its subject matches and its
/// result is that claim.
///
/// # Errors
///
/// Returns an error when the evidence owner cannot build its receipt.
pub(crate) fn from_owner(
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

    struct ClearRetained;

    impl Drop for ClearRetained {
        fn drop(&mut self) {
            super::clear_source_identity();
        }
    }

    fn manifest() -> String {
        let rev = "0123456789abcdef0123456789abcdef01234567";
        format!(
            "[package]\npublish = false\n[dependencies]\nsim-example = {{ version = \"0.1.0\", git = \"https://example.invalid/sim-example.git\", rev = \"{rev}\" }}\n"
        )
    }

    #[test]
    fn a_source_identity_catalog_stays_out_of_the_claims() {
        let _guard = ClearRetained;
        let subject = "subject/tooling-caller";
        let observed =
            sim_conformance_packs::packs::release::observe_source_identity(subject, &manifest())
                .expect("admitted");
        super::retain_source_identity(
            &observed.subject,
            &observed.result,
            observed.catalog.clone(),
            observed.checker.clone(),
            observed.scope.clone(),
            observed.revocation.clone(),
        )
        .expect("retained");
        let (rows, checks) = super::from_owner(subject, &[]).expect("owner");
        assert!(rows.is_empty());
        let checks = checks.expect("catalog");
        assert_eq!(
            checks.scope,
            sim_conformance_evidence::CheckScopeId::from_text("release/source-identity")
                .expect("scope")
        );
        assert!(!observed.result.contains("completion/claim-"));
        let (other_rows, other_checks) = super::from_owner("subject/lantern", &[]).expect("other");
        assert!(other_rows.is_empty());
        assert!(other_checks.is_none());
        let (live_rows, live_checks) = super::from_owner(&"ab".repeat(32), &[]).expect("live");
        assert!(live_rows.is_empty());
        assert!(live_checks.is_none());
    }

    #[test]
    fn a_claim_result_does_not_replace_the_source_identity_catalog() {
        let _guard = ClearRetained;
        let subject = "subject/tooling-caller";
        let observed =
            sim_conformance_packs::packs::release::observe_source_identity(subject, &manifest())
                .expect("admitted");
        super::retain_source_identity(
            &observed.subject,
            &observed.result,
            observed.catalog.clone(),
            observed.checker.clone(),
            observed.scope.clone(),
            observed.revocation.clone(),
        )
        .expect("retained");
        let error = super::retain_source_identity(
            subject,
            "completion/claim-21 git+0123456789abcdef0123456789abcdef01234567:sim-example@0.1.0",
            observed.catalog,
            observed.checker,
            observed.scope,
            observed.revocation,
        )
        .expect_err("refused");
        assert!(
            error
                .to_string()
                .contains("source-identity catalog was refused")
        );
        let (rows, checks) = super::from_owner(subject, &[]).expect("owner");
        assert!(rows.is_empty());
        assert!(checks.is_some());
    }

    #[test]
    fn a_refused_manifest_retains_nothing() {
        let _guard = ClearRetained;
        let subject = "subject/tooling-caller";
        let metadata = "[package]\npublish = false\n[package.metadata]\nsim-example = { version = \"0.1.0\", git = \"https://example.invalid/sim-example.git\", rev = \"0123456789abcdef0123456789abcdef01234567\" }\n";
        let missing =
            match sim_conformance_packs::packs::release::observe_source_identity(subject, metadata)
            {
                Err(error) => error,
                Ok(_) => panic!("metadata must not admit"),
            };
        assert_eq!(missing.code, "missing-source");
        let specimen = match sim_conformance_packs::packs::release::observe_source_identity(
            subject,
            "[package]\npublish = false\nname = \"shade-1\"\n",
        ) {
            Err(error) => error,
            Ok(_) => panic!("specimen must not admit"),
        };
        assert_eq!(specimen.code, "construction-specimen");
        let (rows, checks) = super::from_owner(subject, &[]).expect("owner");
        assert!(rows.is_empty());
        assert!(checks.is_none());
    }

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
