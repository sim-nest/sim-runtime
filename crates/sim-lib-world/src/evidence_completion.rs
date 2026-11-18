// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scoped release admissions for completion tests.
//!
//! Each claim is admitted by the same catalog path the evidence case uses.
//! The subject and scope are the completion binding. This is not a live subject.

use std::collections::BTreeMap;

use sim_conformance_evidence::{
    CheckScopeId, CheckerReceipt, CheckerReceiptId, EvidenceGrade, RevocationStatus,
};
use sim_lib_roadmap_runner::{
    AdmittedEvidence, ProofCatalog, ProofError, ProofLeaf, RetainedCheckerReceipt,
};

/// One scoped release admission. The subject is the named completion world.
pub(crate) struct ScopedRelease {
    /// Catalog that admitted the claims.
    pub catalog: ProofCatalog,
    /// Owner revocation state recorded for the admitted checker receipts.
    pub revocations: BTreeMap<CheckerReceiptId, RevocationStatus>,
    /// Scope those receipts were checked under.
    pub scope: CheckScopeId,
    /// Claim, admitted evidence, and the checker receipt that bound it.
    pub admissions: Vec<(u8, AdmittedEvidence, CheckerReceiptId)>,
}

pub(crate) fn scoped_release(world: &str, scope: &str) -> Result<ScopedRelease, ProofError> {
    scoped_release_results(world, scope, &[])
}

/// Same catalog path as [`scoped_release`], with a replacement checker result.
pub(crate) fn scoped_release_results(
    world: &str,
    scope: &str,
    overrides: &[(u8, &str)],
) -> Result<ScopedRelease, ProofError> {
    let workspace = super::definition(
        "workspace",
        "resolver/lantern",
        "lantern",
        &["resolver/lantern", "path", "lantern", "1.0.0", "."],
        "aaaa1111",
    );
    let generated = super::definition(
        "generate",
        "resolver/lantern",
        "src/generated.rs",
        &["resolver/lantern", "src/generated.rs"],
        "dddd4444",
    );
    let found = [workspace, generated];
    let leaf = ProofLeaf::ArtifactEquality {
        name: "anchor".into(),
        left: b"same".to_vec(),
        right: b"same".to_vec(),
    };
    let mut catalog = ProofCatalog::new([leaf.clone()])?;
    let mut revocations = BTreeMap::new();
    let mut admissions = Vec::with_capacity(25);
    for claim in 1..=25 {
        let fallback = crate::completion::bound_result(claim);
        let result = overrides
            .iter()
            .find(|(number, _)| *number == claim)
            .map(|(_, text)| *text)
            .unwrap_or(fallback.as_str());
        let (next, admitted, checker) =
            admit_one(catalog, &leaf, &found, world, scope, claim, result)?;
        catalog = next;
        revocations.insert(checker.clone(), RevocationStatus::Current);
        admissions.push((claim, admitted, checker));
    }
    Ok(ScopedRelease {
        catalog,
        revocations,
        scope: super::sid(scope)?,
        admissions,
    })
}

fn admit_one(
    catalog: ProofCatalog,
    leaf: &ProofLeaf,
    found: &[sim_lib_roadmap_runner::DiscoveredDefinition],
    world: &str,
    scope: &str,
    claim: u8,
    result: &str,
) -> Result<(ProofCatalog, AdmittedEvidence, CheckerReceiptId), ProofError> {
    let mut shape = super::shape_from(found)?;
    shape.subject = super::sid(world)?;
    shape.scope = super::sid(scope)?;
    shape.environment = format!("environment/fixture-claim-{claim}");
    let template = super::claim(&shape, super::sid("receipt/unused")?, "", Vec::new());
    let chosen = [&found[0], &found[1]];
    let bound = super::fixture_bound(leaf, &template, &chosen)?;
    let binding = super::checker_binding_for(scope)?;
    let invocation = binding
        .instantiate(
            super::sid("code/catalog")?,
            super::sid("pack/catalog")?,
            shape.subject.clone(),
            shape.scope.clone(),
            bound,
        )
        .map_err(|error| ProofError::Invalid(error.to_string()))?;
    let receipt = CheckerReceipt::passing(
        &invocation,
        super::sid(result)?,
        EvidenceGrade::Release,
        super::sid(&format!("provenance/fixture-claim-{claim}"))?,
        super::sid("policy/catalog")?,
        super::sid("support/checker")?,
        RevocationStatus::Current,
    )
    .map_err(|error| ProofError::Invalid(error.to_string()))?;
    let id = receipt.id().clone();
    let mut catalog = catalog.with_checker_receipt(
        RetainedCheckerReceipt::new(binding, invocation, receipt),
        RevocationStatus::Current,
    )?;
    let admitted = super::admit(&mut catalog, &shape, &id, "", Vec::new())?;
    Ok((catalog, admitted, id))
}

/// One claim whose support was checked by a different owner checker.
///
/// The returned checker id is that support owner. The claim receipt keeps
/// its own scope and checker.
pub(crate) fn scoped_release_with_foreign_support(
    world: &str,
    scope: &str,
    claim: u8,
) -> Result<(ScopedRelease, CheckerReceiptId), ProofError> {
    let mut release = scoped_release(world, scope)?;
    let workspace = super::definition(
        "workspace",
        "resolver/lantern",
        "lantern",
        &["resolver/lantern", "path", "lantern", "1.0.0", "."],
        "aaaa1111",
    );
    let generated = super::definition(
        "generate",
        "resolver/lantern",
        "src/generated.rs",
        &["resolver/lantern", "src/generated.rs"],
        "dddd4444",
    );
    let found = [workspace, generated];
    let leaf = release.catalog.leaf("anchor")?.clone();
    let (catalog, support_admitted, support_checker) = admit_one(
        release.catalog,
        &leaf,
        &found,
        world,
        "scope/support-owner",
        0,
        "support/owner-b",
    )?;
    release.catalog = catalog;
    let checker = release
        .admissions
        .iter()
        .find(|(number, _, _)| *number == claim)
        .map(|(_, _, checker)| checker.clone())
        .expect("selected claim");
    let admitted = admit_supported(
        &mut release.catalog,
        &found,
        world,
        scope,
        claim,
        &checker,
        vec![support_admitted.id],
    )?;
    for row in &mut release.admissions {
        if row.0 == claim {
            row.1 = admitted;
            break;
        }
    }
    release
        .revocations
        .insert(support_checker.clone(), RevocationStatus::Current);
    Ok((release, support_checker))
}

fn admit_supported(
    catalog: &mut ProofCatalog,
    found: &[sim_lib_roadmap_runner::DiscoveredDefinition],
    world: &str,
    scope: &str,
    claim: u8,
    checker: &CheckerReceiptId,
    supports: Vec<sim_lib_roadmap_runner::EvidenceReceiptId>,
) -> Result<AdmittedEvidence, ProofError> {
    let mut shape = super::shape_from(found)?;
    shape.subject = super::sid(world)?;
    shape.scope = super::sid(scope)?;
    shape.environment = format!("environment/fixture-claim-{claim}");
    super::admit(catalog, &shape, checker, "", supports)
}
