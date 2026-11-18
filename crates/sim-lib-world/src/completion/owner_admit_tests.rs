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
        Self
    }
}

impl Drop for ClearRetained {
    fn drop(&mut self) {
        super::clear_source_identity();
        super::clear_journal_completion();
        super::clear_owner_claims();
    }
}

fn digest_list() -> String {
    (0..29)
        .map(|index| format!("phase/{index:02} {index:064x}\n"))
        .collect()
}

fn manifest() -> String {
    let rev = "0123456789abcdef0123456789abcdef01234567";
    format!(
        "[package]\npublish = false\n[dependencies]\nsim-example = {{ version = \"0.1.0\", git = \"https://example.invalid/sim-example.git\", rev = \"{rev}\" }}\n"
    )
}

#[test]
fn a_source_identity_catalog_stays_out_of_the_claims() {
    let _guard = ClearRetained::arm();
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
    let _guard = ClearRetained::arm();
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
    let _guard = ClearRetained::arm();
    let subject = "subject/tooling-caller";
    let metadata = "[package]\npublish = false\n[package.metadata]\nsim-example = { version = \"0.1.0\", git = \"https://example.invalid/sim-example.git\", rev = \"0123456789abcdef0123456789abcdef01234567\" }\n";
    let missing =
        match sim_conformance_packs::packs::release::observe_source_identity(subject, metadata) {
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
    let export = crate::journal::executed_export().expect("replay");
    let probe = "\
journal-export-v1
object 09
entry 0 world fact 0
entry 1 world fact 0
telemetry side-channel
";
    super::retain_journal_completion(&subject, probe).expect("unbound keeps the catalog");
    super::retain_journal_completion("subject/other", &export)
        .expect("other subject keeps the catalog");
    let (rows, checks) = super::from_owner(&subject, &[]).expect("unbound");
    assert!(rows.is_empty());
    assert!(checks.is_none());
    super::retain_journal_completion(&subject, &export).expect("replay");
    let (rows, checks) = super::from_owner(&subject, &[]).expect("owner");
    assert_eq!(
        rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
        vec![17]
    );
    assert_eq!(
        checks.expect("catalog").scope,
        sim_conformance_evidence::CheckScopeId::from_text("completion/replay").expect("scope")
    );
    let observed =
        sim_conformance_packs::packs::release::observe_source_identity(&subject, &manifest())
            .expect("source");
    super::retain_source_identity(
        &observed.subject,
        &observed.result,
        observed.catalog,
        observed.checker,
        observed.scope,
        observed.revocation,
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
fn a_fabricated_digest_list_retains_no_claim() {
    let _guard = ClearRetained::arm();
    let subject = "subject/replay-example";
    match sim_conformance_packs::packs::release::observe_journal_completion(subject, &digest_list())
    {
        Err(error) => assert_eq!(error.code, "fabricated-digest-list"),
        Ok(_) => panic!("digest list admitted a receipt"),
    }
    super::retain_journal_completion(subject, &digest_list()).expect("refusal keeps the catalog");
    super::retain_journal_completion(subject, "shade-1\n").expect("specimen keeps the catalog");
    let (rows, checks) = super::from_owner(subject, &[]).expect("owner");
    assert!(rows.is_empty());
    assert!(checks.is_none());
}

#[test]
fn owner_claims_continue_past_the_journal() {
    let _guard = ClearRetained::arm();
    let subject = crate::prove::admitted_subject().expect("subject");
    let export = crate::journal::executed_export().expect("replay");
    super::retain_journal_completion(&subject, &export).expect("replay");
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
        sim_conformance_evidence::RevocationStatus::Current,
    );
    super::retain_owner_claim(
        &subject,
        claim,
        admitted,
        checker.clone(),
        scoped.catalog,
        scoped.scope,
        sim_conformance_evidence::RevocationStatus::Current,
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
            status,
        );
        let (rows, _) = super::from_owner(&subject, &[]).expect("owner");
        assert_eq!(
            rows.iter().map(|row| row.claim).collect::<Vec<_>>(),
            vec![17]
        );
    }
}
