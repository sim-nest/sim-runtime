// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::super::CheckerCatalog;
use super::claim_from_result;

struct ClearRetained;

impl ClearRetained {
    fn arm() -> Self {
        super::clear_source_identity();
        super::clear_journal_completion();
        super::clear_owner_claims();
        super::clear_supplied_owner_claims();
        Self
    }
}

impl Drop for ClearRetained {
    fn drop(&mut self) {
        super::clear_source_identity();
        super::clear_journal_completion();
        super::clear_owner_claims();
        super::clear_supplied_owner_claims();
    }
}

const SOURCE_RESULT: &str =
    "source-identity git+0123456789abcdef0123456789abcdef01234567:sim-example@0.1.0";

fn source_catalog(
    subject: &str,
) -> (
    sim_lib_roadmap_runner::ProofCatalog,
    sim_conformance_evidence::CheckerReceiptId,
    sim_conformance_evidence::CheckScopeId,
) {
    let scoped =
        crate::evidence::scoped_release(subject, "release/source-identity").expect("source");
    let checker = scoped
        .admissions
        .iter()
        .find(|(claim, _, _)| *claim == 1)
        .expect("checker")
        .2
        .clone();
    (scoped.catalog, checker, scoped.scope)
}

fn replay_catalog(
    subject: &str,
) -> (
    sim_lib_roadmap_runner::ProofCatalog,
    sim_lib_roadmap_runner::AdmittedEvidence,
    sim_conformance_evidence::CheckerReceiptId,
    sim_conformance_evidence::CheckScopeId,
) {
    let scoped = crate::evidence::scoped_release(subject, "completion/replay").expect("replay");
    let (_, admitted, checker) = scoped
        .admissions
        .into_iter()
        .find(|(claim, _, _)| *claim == 17)
        .expect("claim 17");
    (scoped.catalog, admitted, checker, scoped.scope)
}

fn retain_replay(subject: &str) {
    let (catalog, admitted, checker, scope) = replay_catalog(subject);
    super::retain_journal_completion(
        subject,
        "completion/claim-17",
        catalog,
        admitted,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("retained replay");
}

#[test]
fn a_source_identity_catalog_stays_out_of_the_claims() {
    let _guard = ClearRetained::arm();
    let subject = "subject/tooling-caller";
    let (catalog, checker, scope) = source_catalog(subject);
    super::retain_source_identity(
        subject,
        SOURCE_RESULT,
        catalog,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Current,
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
    assert!(!SOURCE_RESULT.contains("completion/claim-"));
    let (other_rows, other_checks) = super::from_owner("subject/lantern", &[]).expect("other");
    assert!(other_rows.is_empty());
    assert!(other_checks.is_none());
    let (live_rows, live_checks) = super::from_owner(&"ab".repeat(32), &[]).expect("live");
    assert!(live_rows.is_empty());
    assert!(live_checks.is_none());
}

#[test]
fn a_claim_result_does_not_replace_the_source_identity_catalog() {
    let _guard = ClearRetained::arm();
    let subject = "subject/tooling-caller";
    let (catalog, checker, scope) = source_catalog(subject);
    super::retain_source_identity(
        subject,
        SOURCE_RESULT,
        catalog.clone(),
        checker.clone(),
        scope.clone(),
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("retained");
    let error = super::retain_source_identity(
        subject,
        "completion/claim-21 git+0123456789abcdef0123456789abcdef01234567:sim-example@0.1.0",
        catalog,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Current,
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
fn a_non_identity_result_retains_nothing() {
    let _guard = ClearRetained::arm();
    let subject = "subject/tooling-caller";
    let (catalog, checker, scope) = source_catalog(subject);
    let error = super::retain_source_identity(
        subject,
        "shade-1",
        catalog,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("source-identity catalog was refused")
    );
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
    let scoped =
        crate::evidence::scoped_release("world-final", "scope/completion").expect("scoped fixture");
    let checks = CheckerCatalog {
        catalog: scoped.catalog,
        scope: scoped.scope,
        revocations: scoped.revocations,
        owners: std::collections::BTreeMap::new(),
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
        owners: std::collections::BTreeMap::new(),
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

#[test]
fn an_executed_journal_admits_only_claim_17() {
    let _guard = ClearRetained::arm();
    let subject = crate::prove::admitted_subject().expect("subject");
    let (catalog, admitted, checker, scope) = replay_catalog(&subject);
    super::retain_journal_completion(
        &subject,
        "completion/claim-1",
        catalog.clone(),
        admitted.clone(),
        checker.clone(),
        scope.clone(),
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("wrong result keeps the catalog");
    super::retain_journal_completion(
        "subject/other",
        "completion/claim-17",
        catalog.clone(),
        admitted.clone(),
        checker.clone(),
        scope.clone(),
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("other subject is not this subject");
    let (rows, checks) = super::from_owner(&subject, &[]).expect("unbound");
    assert!(rows.is_empty());
    assert!(checks.is_none());
    super::retain_journal_completion(
        &subject,
        "completion/claim-17",
        catalog,
        admitted,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("replay");
    let (rows, checks) = super::from_owner(&subject, &[]).expect("owner");
    assert_eq!(
        rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
        vec![17]
    );
    assert_eq!(
        checks.expect("catalog").scope,
        sim_conformance_evidence::CheckScopeId::from_text("completion/replay").expect("scope")
    );
    let (source_catalog, source_checker, source_scope) = source_catalog(&subject);
    super::retain_source_identity(
        &subject,
        SOURCE_RESULT,
        source_catalog,
        source_checker,
        source_scope,
        sim_conformance_evidence::RevocationStatus::Current,
    )
    .expect("retained");
    let (rows, checks) = super::from_owner(&subject, &[]).expect("both");
    assert_eq!(
        rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
        vec![17]
    );
    assert_eq!(
        checks.expect("replay catalog").scope,
        sim_conformance_evidence::CheckScopeId::from_text("completion/replay").expect("scope")
    );
    super::clear_journal_completion();
    let (rows, checks) = super::from_owner(&subject, &[]).expect("source only");
    assert!(rows.is_empty());
    assert_eq!(
        checks.expect("source catalog").scope,
        sim_conformance_evidence::CheckScopeId::from_text("release/source-identity")
            .expect("scope")
    );
}

#[test]
fn a_revoked_replay_retains_no_claim() {
    let _guard = ClearRetained::arm();
    let subject = "subject/replay-example";
    let (catalog, admitted, checker, scope) = replay_catalog(subject);
    super::retain_journal_completion(
        subject,
        "completion/claim-17",
        catalog.clone(),
        admitted.clone(),
        checker.clone(),
        scope.clone(),
        sim_conformance_evidence::RevocationStatus::Revoked,
    )
    .expect("revoked keeps the catalog");
    super::retain_journal_completion(
        subject,
        "completion/claim-17",
        catalog,
        admitted,
        checker,
        scope,
        sim_conformance_evidence::RevocationStatus::Unknown,
    )
    .expect("unknown keeps the catalog");
    let (rows, checks) = super::from_owner(subject, &[]).expect("owner");
    assert!(rows.is_empty());
    assert!(checks.is_none());
}

#[test]
fn owner_claims_continue_past_the_journal() {
    let _guard = ClearRetained::arm();
    let subject = crate::prove::admitted_subject().expect("subject");
    retain_replay(&subject);
    let scoped = crate::evidence::scoped_release(&subject, "scope/owner-claim-1").expect("scoped");
    let (claim, admitted, checker) = scoped
        .admissions
        .into_iter()
        .find(|(claim, _, _)| *claim == 1)
        .expect("claim 1");
    let keep = (
        admitted.clone(),
        checker.clone(),
        scoped.catalog.clone(),
        scoped.scope.clone(),
    );
    super::retain_owner_claim(
        "subject/other",
        2,
        admitted.clone(),
        checker.clone(),
        scoped.catalog.clone(),
        scoped.scope.clone(),
        std::collections::BTreeMap::from([(
            checker.clone(),
            sim_conformance_evidence::RevocationStatus::Current,
        )]),
    );
    super::retain_owner_claim(
        &subject,
        claim,
        admitted,
        checker.clone(),
        scoped.catalog,
        scoped.scope,
        std::collections::BTreeMap::from([(
            checker.clone(),
            sim_conformance_evidence::RevocationStatus::Current,
        )]),
    );
    let (rows, checks) = super::from_owner(&subject, &[]).expect("owner");
    assert_eq!(
        rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
        vec![1, 17]
    );
    assert_ne!(rows[0].checker, rows[1].checker);
    let checks = checks.expect("catalog");
    assert_eq!(
        checks.scope,
        sim_conformance_evidence::CheckScopeId::from_text("completion/replay").expect("scope")
    );
    assert!(checks.owners.contains_key(&rows[0].checker));
    assert!(!checks.owners.contains_key(&rows[1].checker));
    for status in [
        sim_conformance_evidence::RevocationStatus::Revoked,
        sim_conformance_evidence::RevocationStatus::Unknown,
    ] {
        super::clear_owner_claims();
        super::retain_owner_claim(
            &subject,
            claim,
            keep.0.clone(),
            keep.1.clone(),
            keep.2.clone(),
            keep.3.clone(),
            std::collections::BTreeMap::from([(keep.1.clone(), status)]),
        );
        let (rows, _) = super::from_owner(&subject, &[]).expect("owner");
        assert_eq!(
            rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
            vec![17]
        );
    }
}

fn supply_foreign_support(
    subject: &str,
    status: sim_conformance_evidence::RevocationStatus,
) -> sim_conformance_evidence::CheckerReceiptId {
    let (mut release, support_checker) =
        crate::evidence::scoped_release_with_foreign_support(subject, "scope/owner-support", 6)
            .expect("foreign support");
    release.revocations.insert(support_checker.clone(), status);
    let (claim, admitted, checker) = release
        .admissions
        .into_iter()
        .find(|(number, _, _)| *number == 6)
        .expect("claim 6");
    super::clear_owner_claims();
    super::supply_owner_claim(super::SuppliedOwnerClaim {
        subject: subject.to_owned(),
        claim,
        admitted,
        checker,
        catalog: release.catalog,
        scope: release.scope,
        revocations: release.revocations,
    });
    support_checker
}

fn admission_claims(datum: &sim_kernel::Datum) -> Vec<String> {
    let sim_kernel::Datum::Node { fields, .. } = datum else {
        panic!("catalog node");
    };
    let value = fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == "admissions")
        .map(|(_, value)| value)
        .expect("admissions");
    let sim_kernel::Datum::Vector(items) = value else {
        panic!("admissions list");
    };
    items
        .iter()
        .map(|item| match item {
            sim_kernel::Datum::String(text) => text.clone(),
            other => panic!("admission {other:?}"),
        })
        .collect()
}

#[test]
fn supplied_foreign_support_survives_assembly() {
    let _guard = ClearRetained::arm();
    let subject = crate::prove::admitted_subject().expect("subject");
    let current = sim_conformance_evidence::RevocationStatus::Current;
    let support = supply_foreign_support(&subject, current.clone());
    let datum = super::super::assemble::catalog_datum("", None).expect("catalog");
    assert_eq!(admission_claims(&datum), vec!["6".to_owned()]);
    let (rows, checks) = super::from_owner(&subject, &[]).expect("owner");
    assert_eq!(
        rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
        vec![6]
    );
    let checks = checks.expect("catalog");
    assert_eq!(checks.revocations.get(&support), Some(&current));

    for status in [
        sim_conformance_evidence::RevocationStatus::Revoked,
        sim_conformance_evidence::RevocationStatus::Unknown,
    ] {
        let support = supply_foreign_support(&subject, status.clone());
        let datum = super::super::assemble::catalog_datum("", None).expect("catalog");
        assert!(admission_claims(&datum).iter().all(|claim| claim != "6"));
        let (rows, checks) = super::from_owner(&subject, &[]).expect("owner");
        assert!(rows.iter().all(|row| row.claim != 6));
        let checks = checks.expect("catalog");
        assert_eq!(checks.revocations.get(&support), Some(&status));
        assert!(checks.revocations.values().any(|found| *found == current));
    }
}
