// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_conformance_evidence::{
    CheckerReceiptId as OwnerCheckerReceiptId, EvidenceGrade, RevocationStatus,
};
use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{AdmittedEvidence, EvidenceKeyId, EvidenceReceiptId};

use super::{
    ArtifactPostimage, AuthorityTransfer, AuthorityTransferId, CheckerCatalog, ClaimAdmission,
    CompletionSeal, EvidenceCatalog, EvidenceSetId, LinuxBoundary, RebuiltExecution,
    ReleaseObligation, SelectedRoadmap, SelectedRoadmapId, WorldSealId, derive_completion,
    derive_fresh_record,
};

fn roadmap_id() -> SelectedRoadmapId {
    SelectedRoadmapId {
        value: "roadmap-selected".to_owned(),
    }
}

fn world_id() -> WorldSealId {
    WorldSealId {
        value: "world-final".to_owned(),
    }
}

fn export(project: &str) -> String {
    format!(
        "\
export world/succession-export-v1
package sim-incremental-core 0.5.2 registry
package sim-storage-port 0.2.2 registry
package sim-lib-world 0.1.1 registry
project {project}
input provider/widget observed widget-body
input remote/hostile unavailable -
"
    )
}

fn independent() -> Datum {
    crate::fresh::run(
        &export("harbor"),
        "independent harbor provider/marker observed marker-next",
    )
    .expect("fresh independent change")
}

fn shade() -> Datum {
    crate::fresh::run(
        &export("lantern"),
        "shade-1 lantern provider/shade observed shade-next",
    )
    .expect("fresh construction specimen")
}

fn proved() -> (
    SelectedRoadmap,
    RebuiltExecution,
    EvidenceCatalog,
    CheckerCatalog,
    Vec<AuthorityTransfer>,
) {
    let roadmap = roadmap_id();
    let world = world_id();
    let selected = SelectedRoadmap {
        id: roadmap.clone(),
        final_world: world.clone(),
        human_selection: "selection-record".to_owned(),
        begin_authority: "begin-record".to_owned(),
        source_digest: "source-digest".to_owned(),
        sibling_dependency: None,
    };
    let state = RebuiltExecution {
        final_world: world.clone(),
        replay_digest: "replay-digest".to_owned(),
        frontier: Vec::new(),
        continuation: None,
        second_pass_work: 0,
        leases: Vec::new(),
        uncertain_effects: Vec::new(),
        artifacts: vec![ArtifactPostimage {
            owner: "world-generator".to_owned(),
            postimage: "image-1".to_owned(),
        }],
    };
    // Scoped fixture: release evidence for world-final and scope/completion,
    // admitted through the lantern catalog path. This is not live NV12.
    let scoped = crate::evidence::scoped_release(world.value.as_str(), "scope/completion")
        .expect("scoped release fixture");
    let admissions = scoped
        .admissions
        .into_iter()
        .map(|(claim, admitted, checker)| ClaimAdmission {
            claim,
            admitted,
            checker,
        })
        .collect();
    let evidence = EvidenceCatalog {
        id: EvidenceSetId {
            value: "evidence-set".to_owned(),
        },
        roadmap,
        final_world: world,
        source_digest: "source-digest".to_owned(),
        replay_digest: "replay-digest".to_owned(),
        admissions,
        fresh: independent(),
        releases: vec![ReleaseObligation {
            name: "sim-lib-doc-core".to_owned(),
            registry_published: true,
            source_release: String::new(),
            order: 0,
        }],
        linux: LinuxBoundary::Absent,
        model_provider_credential: false,
    };
    let transfer = AuthorityTransfer {
        id: AuthorityTransferId {
            value: "transfer-portable".to_owned(),
        },
        scope: "portable".to_owned(),
        caller_zero: true,
        retained_role: None,
        native_credential: false,
    };
    (
        selected,
        state,
        evidence,
        CheckerCatalog {
            catalog: scoped.catalog,
            scope: scoped.scope,
            revocations: scoped.revocations,
            owners: std::collections::BTreeMap::new(),
        },
        vec![transfer],
    )
}

fn derive(
    inputs: &(
        SelectedRoadmap,
        RebuiltExecution,
        EvidenceCatalog,
        CheckerCatalog,
        Vec<AuthorityTransfer>,
    ),
) -> Result<CompletionSeal, Vec<super::CompletionGap>> {
    derive_completion(&inputs.0, &inputs.1, &inputs.2, &inputs.3, &inputs.4)
}

fn reasons(gaps: &[super::CompletionGap]) -> Vec<&str> {
    gaps.iter().map(|gap| gap.reason.as_str()).collect()
}

fn exact_unpublished() -> Vec<ReleaseObligation> {
    [
        (
            "sim-codec-mspdi",
            "fdb39e34ede165c1e261f518e56a338fc5a3ab33",
        ),
        ("sim-ledger", "a9a376f1376a19f153011e29257f9af7465a323f"),
        (
            "sim-ledger-store-port",
            "a9a376f1376a19f153011e29257f9af7465a323f",
        ),
        (
            "sim-lib-doc-site",
            "bb8279c1bcbfe14aac8abe75f4bb03f14d29480c",
        ),
        ("sim-lib-gantt", "bb8279c1bcbfe14aac8abe75f4bb03f14d29480c"),
        ("sim-lib-sheet", "bb8279c1bcbfe14aac8abe75f4bb03f14d29480c"),
        ("sim-site-dalux", "fdb39e34ede165c1e261f518e56a338fc5a3ab33"),
        (
            "sim-site-powerproject",
            "fdb39e34ede165c1e261f518e56a338fc5a3ab33",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(order, (name, commit))| ReleaseObligation {
        name: name.to_owned(),
        registry_published: false,
        source_release: format!("git+{commit}:{name}@0.1.0"),
        order: u16::try_from(order).expect("order"),
    })
    .collect()
}

fn unpublished() -> Vec<ReleaseObligation> {
    [
        "sim-codec-mspdi",
        "sim-ledger",
        "sim-ledger-store-port",
        "sim-lib-doc-site",
        "sim-lib-gantt",
        "sim-lib-sheet",
        "sim-site-dalux",
        "sim-site-powerproject",
    ]
    .into_iter()
    .enumerate()
    .map(|(order, name)| ReleaseObligation {
        name: name.to_owned(),
        registry_published: false,
        source_release: String::new(),
        order: u16::try_from(order).expect("order"),
    })
    .collect()
}

/// Scoped unit fixture. `world-final` and the lantern catalog are not live NV12.
#[test]
fn a_scoped_fixture_returns_a_stable_portable_seal() {
    let inputs = proved();
    let first = derive(&inputs).expect("scoped fixture");
    let second = derive(&inputs).expect("scoped fixture");
    assert_eq!(first, second);
    assert_eq!(first.final_world.value, "world-final");
    assert_eq!(first.domain, super::SealDomain::PortableClosed);
    assert_eq!(first.claims.len(), 25);
    assert_eq!(first.claims[22].number, 23);
    let mut keys: Vec<_> = first.claims.iter().map(|claim| claim.key.clone()).collect();
    let width = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), width);
    assert!(keys.iter().all(|key| !key.is_empty()));
    assert!(!first.digest.value.is_empty());
    assert_eq!(first.checker_receipts.len(), 25);
}

#[test]
fn a_changed_fact_changes_the_digest() {
    let inputs = proved();
    let sealed = derive(&inputs).expect("proved subject");
    let mut changed = proved();
    changed.2.id.value = "evidence-set-2".to_owned();
    let next = derive(&changed).expect("changed evidence identity");
    assert_ne!(sealed.digest, next.digest);
    assert_eq!(next.evidence.value, "evidence-set-2");
}

#[test]
fn shade_1_stays_a_construction_specimen_gap() {
    let mut inputs = proved();
    inputs.2.fresh = shade();
    let gaps = derive(&inputs).expect_err("specimen");
    let found = reasons(&gaps);
    assert!(found.contains(&"construction-specimen"));
    assert!(!found.iter().any(|reason| reason.contains("PortableClosed")));
}

#[test]
fn unpublished_releases_stay_gaps_on_an_otherwise_proved_subject() {
    let mut inputs = proved();
    inputs.2.releases = unpublished();
    let gaps = derive(&inputs).expect_err("unpublished");
    let found = reasons(&gaps);
    for name in [
        "sim-codec-mspdi",
        "sim-ledger",
        "sim-ledger-store-port",
        "sim-lib-doc-site",
        "sim-lib-gantt",
        "sim-lib-sheet",
        "sim-site-dalux",
        "sim-site-powerproject",
    ] {
        let expected = format!("unpublished-git-source:{name}");
        assert!(found.contains(&expected.as_str()));
    }
    assert!(
        !found
            .iter()
            .any(|reason| reason.contains("sim-lib-doc-core"))
    );
}

fn admitted_for(
    releases: &[ReleaseObligation],
) -> (
    SelectedRoadmap,
    RebuiltExecution,
    EvidenceCatalog,
    CheckerCatalog,
    Vec<AuthorityTransfer>,
) {
    let mut inputs = proved();
    let result = super::check::release_claim_result(releases);
    let world = inputs.0.final_world.value.clone();
    let scoped = crate::evidence::scoped_release_results(
        &world,
        "scope/completion",
        &[(21, result.as_str())],
    )
    .expect("covering admission");
    inputs.2.admissions = scoped
        .admissions
        .into_iter()
        .map(|(claim, admitted, checker)| ClaimAdmission {
            claim,
            admitted,
            checker,
        })
        .collect();
    inputs.2.releases = releases.to_vec();
    inputs.3.catalog = scoped.catalog;
    inputs.3.scope = scoped.scope;
    inputs.3.revocations = scoped.revocations;
    inputs
}

#[test]
fn exact_git_source_releases_discharge_unpublished_tooling() {
    let inputs = admitted_for(&exact_unpublished());
    let sealed = derive(&inputs).expect("exact source releases");
    assert_eq!(sealed.claims[20].number, 21);
}

#[test]
fn a_changed_source_identity_keeps_the_admitted_evidence_a_gap() {
    let releases = exact_unpublished();
    let inputs = admitted_for(&releases);
    let admitted = inputs.2.admissions.clone();
    let mut changed = inputs;
    changed.2.releases[0].source_release =
        "git+0123456789abcdef0123456789abcdef01234567:sim-codec-mspdi@0.1.0".to_owned();
    assert_eq!(changed.2.admissions, admitted);
    let gaps = derive(&changed).expect_err("uncovered revision");
    assert!(reasons(&gaps).contains(&"unpublished-git-source:sim-codec-mspdi"));
}

#[test]
fn a_path_or_mismatched_source_stays_unpublished() {
    let mut inputs = proved();
    inputs.2.releases = exact_unpublished();
    inputs.2.releases[0].source_release = "path+../sim-office:sim-codec-mspdi@0.1.0".to_owned();
    inputs.2.releases[1].source_release =
        "git+a9a376f1376a19f153011e29257f9af7465a323f:sim-ledger-other@0.1.0".to_owned();
    let gaps = derive(&inputs).expect_err("inexact source");
    let found = reasons(&gaps);
    assert!(found.contains(&"unpublished-git-source:sim-codec-mspdi"));
    assert!(found.contains(&"unpublished-git-source:sim-ledger"));
}

#[test]
fn a_published_release_adds_no_git_source_gap() {
    let inputs = proved();
    let sealed = derive(&inputs).expect("published obligation");
    assert_eq!(sealed.claims[20].number, 21);
}

#[test]
fn a_warm_shade_1_checkout_stays_a_gap() {
    let body = export("lantern");
    let record = crate::fresh::run(
        &format!("{body}checkout\n"),
        "shade-1 lantern provider/shade observed shade-next",
    )
    .expect("warm");
    let mut inputs = proved();
    inputs.2.fresh = record;
    let gaps = derive(&inputs).expect_err("warm");
    assert!(reasons(&gaps).contains(&"warm-state"));
}

#[test]
fn a_linux_fresh_claim_stays_a_gap() {
    let body = export("harbor");
    let record = crate::fresh::run(
        &format!("{body}claim reboot\n"),
        "independent harbor provider/marker observed marker-next",
    )
    .expect("linux");
    let mut inputs = proved();
    inputs.2.fresh = record;
    let gaps = derive(&inputs).expect_err("linux");
    assert!(reasons(&gaps).contains(&"linux-claim"));
}

#[test]
fn an_empty_release_name_stays_a_gap() {
    let mut inputs = proved();
    inputs.2.releases = vec![ReleaseObligation {
        name: "  ".to_owned(),
        registry_published: false,
        source_release: String::new(),
        order: 0,
    }];
    let gaps = derive(&inputs).expect_err("missing name");
    assert!(reasons(&gaps).contains(&"unpublished-git-source:missing-name"));
}

#[test]
fn a_forged_fresh_seal_string_is_not_evidence() {
    let mut inputs = proved();
    inputs.2.fresh = Datum::Node {
        tag: Symbol::qualified("world", "fresh-v1"),
        fields: vec![(Symbol::new("seal"), Datum::String("claimed".to_owned()))],
    };
    let gaps = derive(&inputs).expect_err("forged seal");
    assert!(reasons(&gaps).contains(&"fresh-seal-not-evidence"));
}

#[test]
fn a_missing_claim_stays_unproved() {
    let mut inputs = proved();
    inputs.2.admissions.retain(|row| row.claim != 7);
    let gaps = derive(&inputs).expect_err("missing claim");
    assert!(
        gaps.iter()
            .any(|gap| gap.claim == 7 && gap.reason == "claim-unproved")
    );
}

#[test]
fn an_authentic_label_is_not_proof() {
    let mut inputs = proved();
    inputs.2.admissions = (1..=25)
        .map(|claim| {
            let label = format!("authentic-{claim}");
            ClaimAdmission {
                claim,
                admitted: AdmittedEvidence {
                    id: EvidenceReceiptId::from_text(&label).expect("receipt label"),
                    key: EvidenceKeyId::from_text(&label).expect("key label"),
                    grade: EvidenceGrade::Release,
                },
                checker: OwnerCheckerReceiptId::from_text(&label).expect("checker label"),
            }
        })
        .collect();
    let mut checks = CheckerCatalog::without_evidence();
    for row in &inputs.2.admissions {
        checks
            .revocations
            .insert(row.checker.clone(), RevocationStatus::Current);
    }
    inputs.3 = checks;
    let gaps = derive(&inputs).expect_err("authentic label");
    assert!(reasons(&gaps).contains(&"evidence-not-verified"));
}

#[test]
fn a_normative_sibling_blocks_the_seal() {
    let mut inputs = proved();
    inputs.0.sibling_dependency = Some("sibling-roadmap".to_owned());
    let gaps = derive(&inputs).expect_err("sibling");
    assert!(reasons(&gaps).contains(&"normative-sibling"));
}

#[test]
fn a_mismatched_final_world_blocks_the_seal() {
    let mut inputs = proved();
    inputs.1.final_world = WorldSealId {
        value: "other-world".to_owned(),
    };
    let gaps = derive(&inputs).expect_err("world");
    assert!(reasons(&gaps).contains(&"final-subject-mismatch"));
}

#[test]
fn a_copied_admission_on_an_empty_catalog_is_not_proof() {
    let mut inputs = proved();
    inputs.3 = CheckerCatalog::without_evidence();
    let gaps = derive(&inputs).expect_err("copied admission");
    assert!(reasons(&gaps).contains(&"unknown-revocation"));
}

#[test]
fn unrelated_evidence_does_not_prove_the_selected_subject() {
    let mut inputs = proved();
    let foreign =
        crate::evidence::scoped_release("other-world", "scope/other").expect("foreign fixture");
    inputs.2.admissions = foreign
        .admissions
        .into_iter()
        .map(|(claim, admitted, checker)| ClaimAdmission {
            claim,
            admitted,
            checker,
        })
        .collect();
    inputs.3.catalog = foreign.catalog;
    inputs.3.revocations = foreign.revocations;
    let gaps = derive(&inputs).expect_err("unrelated");
    assert!(reasons(&gaps).contains(&"evidence-unrelated"));
}

#[test]
fn a_relabelled_admission_does_not_prove_another_claim() {
    let mut inputs = proved();
    let source = inputs
        .2
        .admissions
        .iter()
        .find(|row| row.claim == 1)
        .expect("claim 1")
        .clone();
    let target = inputs
        .2
        .admissions
        .iter_mut()
        .find(|row| row.claim == 7)
        .expect("claim 7");
    target.admitted = source.admitted;
    target.checker = source.checker;
    let gaps = derive(&inputs).expect_err("relabelled");
    assert!(
        gaps.iter()
            .any(|gap| gap.claim == 7 && gap.reason == "claim-relabelled")
    );
}

#[test]
fn a_swapped_admission_does_not_prove_the_kept_checker() {
    let mut inputs = proved();
    let admitted = inputs
        .2
        .admissions
        .iter()
        .find(|row| row.claim == 1)
        .expect("claim 1")
        .admitted
        .clone();
    let target = inputs
        .2
        .admissions
        .iter_mut()
        .find(|row| row.claim == 7)
        .expect("claim 7");
    target.admitted = admitted;
    let gaps = derive(&inputs).expect_err("swapped admission");
    assert!(
        gaps.iter()
            .any(|gap| gap.claim == 7 && gap.reason == "evidence-not-verified")
    );
}

#[test]
fn another_owner_scope_is_not_forced_into_the_catalog_label() {
    let mut inputs = proved();
    inputs.3.scope =
        sim_conformance_evidence::CheckScopeId::from_text("scope/other-owner").expect("scope");
    let seal = derive(&inputs).expect("receipt scopes");
    assert_eq!(seal.domain, super::SealDomain::PortableClosed);
    assert_eq!(seal.claims.len(), 25);
}

#[test]
fn support_from_another_owner_checker_is_release_evidence() {
    let mut inputs = proved();
    let (supported, support_checker) =
        crate::evidence::scoped_release_with_foreign_support("world-final", "scope/completion", 6)
            .expect("foreign support");
    inputs.2.admissions = supported
        .admissions
        .into_iter()
        .map(|(claim, admitted, checker)| ClaimAdmission {
            claim,
            admitted,
            checker,
        })
        .collect();
    inputs.3.catalog = supported.catalog;
    inputs.3.revocations = supported.revocations;
    let seal = derive(&inputs).expect("current support");
    assert_eq!(seal.domain, super::SealDomain::PortableClosed);

    inputs
        .3
        .revocations
        .insert(support_checker.clone(), RevocationStatus::Revoked);
    let revoked = derive(&inputs).expect_err("revoked support");
    assert!(
        revoked
            .iter()
            .any(|gap| gap.claim == 6 && gap.reason == "revoked-support")
    );

    inputs
        .3
        .revocations
        .insert(support_checker, RevocationStatus::Unknown);
    let unknown = derive(&inputs).expect_err("unknown support");
    assert!(
        unknown
            .iter()
            .any(|gap| gap.claim == 6 && gap.reason == "unknown-revocation")
    );
}

#[test]
fn revoked_support_is_not_proof() {
    let mut inputs = proved();
    let checker = inputs
        .2
        .admissions
        .iter()
        .find(|row| row.claim == 6)
        .expect("claim 6")
        .checker
        .clone();
    inputs
        .3
        .revocations
        .insert(checker, RevocationStatus::Revoked);
    let gaps = derive(&inputs).expect_err("revoked");
    assert!(
        gaps.iter()
            .any(|gap| gap.claim == 6 && gap.reason == "revoked-support")
    );
}

#[test]
fn unknown_support_is_not_proof() {
    let mut inputs = proved();
    let checker = inputs
        .2
        .admissions
        .iter()
        .find(|row| row.claim == 6)
        .expect("claim 6")
        .checker
        .clone();
    inputs
        .3
        .revocations
        .insert(checker, RevocationStatus::Unknown);
    let gaps = derive(&inputs).expect_err("unknown");
    assert!(
        gaps.iter()
            .any(|gap| gap.claim == 6 && gap.reason == "unknown-revocation")
    );
}

#[test]
fn a_fresh_record_alone_does_not_seal() {
    let gaps = derive_fresh_record(&independent()).expect_err("fresh alone");
    let found = reasons(&gaps);
    assert!(found.contains(&"claim-unproved"));
    assert!(!found.contains(&"construction-specimen"));
    assert!(!found.contains(&"fresh-seal-not-evidence"));
    assert!(!found.contains(&"fresh-seal-unclaimed"));
}

#[test]
fn the_fresh_leaf_subject_keeps_the_construction_specimen_open() {
    let gaps = derive_fresh_record(&shade()).expect_err("specimen");
    assert!(reasons(&gaps).contains(&"construction-specimen"));
}

#[test]
fn fresh_and_transfer_sources_do_not_carry_the_derivation() {
    for source in [include_str!("../fresh.rs"), include_str!("../transfer.rs")] {
        assert!(!source.contains("derive_completion"));
        assert!(!source.contains("PortableClosed"));
    }
}

#[test]
fn the_final_catalog_assembler_does_not_derive_a_seal() {
    for source in [
        include_str!("assemble.rs"),
        include_str!("../portable_union_catalog.rs"),
    ] {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production.contains("derive_completion"));
        assert!(!production.contains("PortableClosed"));
        assert!(!production.contains("fdb39e34ede165c1e261f518e56a338fc5a3ab33"));
        assert!(!production.contains("bb8279c1bcbfe14aac8abe75f4bb03f14d29480c"));
        assert!(!production.contains("a9a376f1376a19f153011e29257f9af7465a323f"));
    }
}

#[path = "later_fresh_tests.rs"]
mod later_fresh;
