// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure completion derivation for one selected portable subject.
//!
//! A seal is returned only when the roadmap, rebuilt subject, owner
//! catalog, and authority transfers prove claims 1 through 25 on one
//! compatible final subject. A caller grade label is not that proof. The
//! catalog must have admitted each claim at release grade. The fresh
//! command's seal string is not that proof. An open construction specimen,
//! a warm checkout, and a selected release with neither registry
//! publication nor exact source-release evidence stay gaps. Exact
//! source-release evidence is the identity claim 21's verified receipt
//! names. Linux completion stays outside this seal.
//! The final-world catalog is filled from the admitted phase union and
//! the caller release input. Filling that catalog does not derive a seal.

mod assemble;
mod check;
mod fresh_set;
mod owner_admit;
mod seal;

pub(crate) use assemble::catalog_datum;
pub(crate) use fresh_set::hold_notes;
pub use owner_admit::{SuppliedOwnerClaim, supply_owner_claim};

/// Assembles the final-world catalog from receipts already supplied on this thread.
///
/// The caller admits each receipt through [`supply_owner_claim`] before this
/// call. A missing receipt stays unproved. This function does not run a
/// conformance pack and does not derive a seal.
///
/// # Errors
///
/// Returns an error when the release input or the admitted union is unusable.
pub fn final_catalog(releases: &str) -> Result<Datum, crate::WorldError> {
    catalog_datum(releases, None)
}

/// Retains the export and change the fresh command just ran.
///
/// The next [`final_catalog`] assembly takes this pair. This does not
/// derive a seal. The construction specimen stays refused.
pub(crate) fn note_fresh_command(export: &str, change: &str) {
    fresh_set::supply(export, change);
}

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use sim_conformance_evidence::{
    CheckScopeId, CheckerReceiptId as OwnerCheckerReceiptId, RevocationStatus,
};
use sim_kernel::Datum;
use sim_lib_roadmap_runner::{AdmittedEvidence, ProofCatalog, ProofLeaf};

/// Result identity that binds one selected claim inside the owner receipt.
pub(crate) fn bound_result(claim: u8) -> String {
    format!("completion/claim-{claim}")
}

/// Identity of the selected roadmap.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SelectedRoadmapId {
    /// Stable roadmap identity.
    pub value: String,
}

/// Identity of the final world subject.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorldSealId {
    /// Stable world identity.
    pub value: String,
}

/// Identity of one evidence set.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceSetId {
    /// Stable evidence-set identity.
    pub value: String,
}

/// Identity of one checker receipt.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CheckerReceiptId {
    /// Stable receipt identity.
    pub value: String,
}

/// Identity of one authority transfer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AuthorityTransferId {
    /// Stable transfer identity.
    pub value: String,
}

/// Digest of one derived seal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionSealId {
    /// Hex-encoded digest of the proving inputs.
    pub value: String,
}

/// Domain of a derived portable seal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SealDomain {
    /// The selected portable claims are proved.
    PortableClosed,
}

/// One claim carried by a derived seal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionClaim {
    /// Claim number, from 1 through 25.
    pub number: u8,
    /// Stable key bound by the evidence and its checker receipt.
    pub key: String,
}

/// Derived portable seal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionSeal {
    /// Selected roadmap.
    pub roadmap: SelectedRoadmapId,
    /// Final world subject.
    pub final_world: WorldSealId,
    /// Proved claims in numeric order.
    pub claims: Vec<CompletionClaim>,
    /// Evidence set that proved the claims.
    pub evidence: EvidenceSetId,
    /// Checker receipts that bound the claims.
    pub checker_receipts: Vec<CheckerReceiptId>,
    /// Authority transfers read by the derivation.
    pub authority_transfers: Vec<AuthorityTransferId>,
    /// Digest of the proving inputs.
    pub digest: CompletionSealId,
    /// Seal domain. This derivation returns only the portable domain.
    pub domain: SealDomain,
}

/// One selected claim that the supplied inputs do not prove.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CompletionGap {
    /// Claim number from the selected portable seal.
    pub claim: u8,
    /// Stable reason.
    pub reason: String,
}

/// Selected roadmap facts the derivation reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedRoadmap {
    /// Roadmap identity.
    pub id: SelectedRoadmapId,
    /// Final world subject named by the selection.
    pub final_world: WorldSealId,
    /// Authentic human selection record.
    pub human_selection: String,
    /// Authentic begin record. It is a different record from selection.
    pub begin_authority: String,
    /// Digest of the self-contained source.
    pub source_digest: String,
    /// Normative sibling dependency, when one is present.
    pub sibling_dependency: Option<String>,
}

/// One generated artifact and its exact postimage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactPostimage {
    /// Owner of the generated artifact.
    pub owner: String,
    /// Exact postimage of that artifact.
    pub postimage: String,
}

/// Rebuilt final subject the derivation reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebuiltExecution {
    /// Final world subject.
    pub final_world: WorldSealId,
    /// Digest of the deterministic replay.
    pub replay_digest: String,
    /// Open frontier entries.
    pub frontier: Vec<String>,
    /// Remaining continuation, when one is open.
    pub continuation: Option<String>,
    /// Work performed by a second convergence and delivery pass.
    pub second_pass_work: u64,
    /// Live leases.
    pub leases: Vec<String>,
    /// Unresolved outward effects.
    pub uncertain_effects: Vec<String>,
    /// Generated artifacts and their postimages.
    pub artifacts: Vec<ArtifactPostimage>,
}

/// One claim citing evidence the owner catalog admitted.
///
/// The admission grade is not proof. The checker receipt's subject, scope,
/// and result bind this claim. The admitted receipt must be that checker's
/// evidence, and the catalog decides release reuse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimAdmission {
    /// Claim number, from 1 through 25.
    pub claim: u8,
    /// Evidence admitted by the checker catalog.
    pub admitted: AdmittedEvidence,
    /// Owner checker receipt that admitted the evidence.
    pub checker: OwnerCheckerReceiptId,
}

/// One selected release obligation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseObligation {
    /// Package name.
    pub name: String,
    /// Whether the named package is on the registry.
    pub registry_published: bool,
    /// Exact source-release evidence for a package that is not on the registry.
    ///
    /// Claim 21's verified receipt names this revision, package, and version.
    pub source_release: String,
    /// Package version named by the selected release line.
    pub version: String,
    /// Zero-based position in dependency order.
    pub order: u16,
}

/// Linux boundary carried by the evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxBoundary {
    /// Linux is not selected.
    Unselected,
    /// Linux is absent from the portable subject.
    Absent,
    /// Linux was selected and is unfinished.
    SelectedUnfinished,
}

/// Evidence catalog for one final subject.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceCatalog {
    /// Evidence-set identity.
    pub id: EvidenceSetId,
    /// Roadmap identity.
    pub roadmap: SelectedRoadmapId,
    /// Final world subject.
    pub final_world: WorldSealId,
    /// Digest of the self-contained source.
    pub source_digest: String,
    /// Digest of the deterministic replay.
    pub replay_digest: String,
    /// One owner admission for each selected claim.
    pub admissions: Vec<ClaimAdmission>,
    /// Fresh-succession record produced by the fresh command.
    pub fresh: Datum,
    /// Selected release obligations in dependency order.
    pub releases: Vec<ReleaseObligation>,
    /// Linux boundary of this subject.
    pub linux: LinuxBoundary,
    /// Whether a model provider holds a native or publication credential.
    pub model_provider_credential: bool,
}

/// Owner checker catalog for one final subject.
///
/// `revocations` is the owner's revocation state. A missing id is unknown.
/// `Current` is not assumed.
#[derive(Clone, Debug)]
pub struct CheckerCatalog {
    /// Catalog whose admitted evidence is the proof. A grade label is not.
    pub catalog: ProofCatalog,
    /// Scope recorded for `catalog`. Each receipt keeps its own scope.
    pub scope: CheckScopeId,
    /// Authoritative revocation state for checker receipt ids.
    pub revocations: BTreeMap<OwnerCheckerReceiptId, RevocationStatus>,
    /// Catalogs for owner checkers other than `catalog`.
    ///
    /// A checker absent from this map is read from `catalog`.
    pub owners: BTreeMap<OwnerCheckerReceiptId, ProofCatalog>,
}

impl CheckerCatalog {
    /// Catalog with one pure leaf, no admitted evidence, and no revocation state.
    ///
    /// # Panics
    ///
    /// Panics if that leaf or the unproved scope is rejected. Both are fixed tokens.
    pub fn without_evidence() -> Self {
        let catalog = ProofCatalog::new([ProofLeaf::ArtifactEquality {
            name: "unproved".to_owned(),
            left: b"unproved".to_vec(),
            right: b"unproved".to_vec(),
        }])
        .expect("unproved leaf");
        let scope = CheckScopeId::from_text("scope/unproved").expect("unproved scope");
        Self {
            catalog,
            scope,
            revocations: BTreeMap::new(),
            owners: BTreeMap::new(),
        }
    }

    /// Catalog that holds `checker`. The primary catalog is the fallback.
    pub(crate) fn proof_for(&self, checker: &OwnerCheckerReceiptId) -> &ProofCatalog {
        self.owners.get(checker).unwrap_or(&self.catalog)
    }
}

/// One portable authority transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityTransfer {
    /// Transfer identity.
    pub id: AuthorityTransferId,
    /// Scope token. Portable proof uses `portable`.
    pub scope: String,
    /// Whether the caller retains zero authority after the transfer.
    pub caller_zero: bool,
    /// Named ordinary-tool role retained by the caller, when caller authority remains.
    pub retained_role: Option<String>,
    /// Whether this transfer carries a native or publication credential.
    pub native_credential: bool,
}

/// Derives a portable seal from the selected subject.
///
/// `Ok` is returned only when claims 1 through 25 are proved by these
/// inputs on one compatible final subject. Each claim must be a release
/// admission the checker catalog reuses. A grade label, including a
/// matching invented receipt, is not proof. A missing claim, a contradicted
/// subject, an open construction specimen, a warm checkout, a forged fresh
/// seal string, or an unpublished release obligation returns gaps. This
/// function has no override that turns those inputs into a seal.
///
/// # Examples
///
/// ```
/// use sim_kernel::Datum;
/// use sim_lib_world::completion::derive_completion;
/// use sim_lib_world::completion::AuthorityTransfer;
/// use sim_lib_world::completion::CheckerCatalog;
/// use sim_lib_world::completion::EvidenceCatalog;
/// use sim_lib_world::completion::EvidenceSetId;
/// use sim_lib_world::completion::LinuxBoundary;
/// use sim_lib_world::completion::RebuiltExecution;
/// use sim_lib_world::completion::SelectedRoadmap;
/// use sim_lib_world::completion::SelectedRoadmapId;
/// use sim_lib_world::completion::WorldSealId;
///
/// let roadmap = SelectedRoadmapId { value: "roadmap".to_owned() };
/// let world = WorldSealId { value: "world".to_owned() };
/// let selected = SelectedRoadmap {
///     id: roadmap.clone(),
///     final_world: world.clone(),
///     human_selection: String::new(),
///     begin_authority: String::new(),
///     source_digest: String::new(),
///     sibling_dependency: None,
/// };
/// let state = RebuiltExecution {
///     final_world: world.clone(),
///     replay_digest: String::new(),
///     frontier: Vec::new(),
///     continuation: None,
///     second_pass_work: 0,
///     leases: Vec::new(),
///     uncertain_effects: Vec::new(),
///     artifacts: Vec::new(),
/// };
/// let evidence = EvidenceCatalog {
///     id: EvidenceSetId { value: "evidence".to_owned() },
///     roadmap,
///     final_world: world,
///     source_digest: String::new(),
///     replay_digest: String::new(),
///     admissions: Vec::new(),
///     fresh: Datum::Nil,
///     releases: Vec::new(),
///     linux: LinuxBoundary::Unselected,
///     model_provider_credential: false,
/// };
/// let gaps = derive_completion(
///     &selected,
///     &state,
///     &evidence,
///     &CheckerCatalog::without_evidence(),
///     &[] as &[AuthorityTransfer],
/// )
/// .expect_err("empty evidence is a gap");
/// assert!(gaps.iter().any(|gap| gap.reason == "claim-unproved"));
/// ```
pub fn derive_completion(
    selected: &SelectedRoadmap,
    state: &RebuiltExecution,
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    transfers: &[AuthorityTransfer],
) -> Result<CompletionSeal, Vec<CompletionGap>> {
    let mut gaps = check::gaps(selected, state, evidence, checks, transfers);
    if !gaps.is_empty() {
        gaps.sort();
        gaps.dedup();
        return Err(gaps);
    }
    Ok(seal::from_proved(
        selected, state, evidence, checks, transfers,
    ))
}

/// Derives completion for one fresh record and no other claims.
///
/// The fresh leaf uses this to read the construction specimen. The other
/// selected claims are absent, so a fresh record alone does not become a
/// seal. A successful result is left intact for the caller.
pub(crate) fn derive_fresh_record(fresh: &Datum) -> Result<CompletionSeal, Vec<CompletionGap>> {
    let (selected, state, evidence, checks, transfers) = unproved_fresh(fresh);
    derive_completion(&selected, &state, &evidence, &checks, &transfers)
}

fn unproved_fresh(
    fresh: &Datum,
) -> (
    SelectedRoadmap,
    RebuiltExecution,
    EvidenceCatalog,
    CheckerCatalog,
    Vec<AuthorityTransfer>,
) {
    let roadmap = SelectedRoadmapId {
        value: "unproved".to_owned(),
    };
    let world = WorldSealId {
        value: "unproved".to_owned(),
    };
    let selected = SelectedRoadmap {
        id: roadmap.clone(),
        final_world: world.clone(),
        human_selection: String::new(),
        begin_authority: String::new(),
        source_digest: String::new(),
        sibling_dependency: None,
    };
    let state = RebuiltExecution {
        final_world: world.clone(),
        replay_digest: String::new(),
        frontier: vec!["open".to_owned()],
        continuation: None,
        second_pass_work: 0,
        leases: Vec::new(),
        uncertain_effects: Vec::new(),
        artifacts: Vec::new(),
    };
    let evidence = EvidenceCatalog {
        id: EvidenceSetId {
            value: "unproved".to_owned(),
        },
        roadmap,
        final_world: world,
        source_digest: String::new(),
        replay_digest: String::new(),
        admissions: Vec::new(),
        fresh: fresh.clone(),
        releases: Vec::new(),
        linux: LinuxBoundary::Unselected,
        model_provider_credential: false,
    };
    (
        selected,
        state,
        evidence,
        CheckerCatalog::without_evidence(),
        Vec::new(),
    )
}
