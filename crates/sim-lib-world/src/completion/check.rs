// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Claim checks for completion derivation.
//!
//! A claim is proved only when the owner catalog reuses a release receipt
//! for that claim's admitted key and that receipt's checker is the one
//! whose claim, subject, scope, and revocation state were validated.
//! The admission's grade field is not read.

use std::collections::BTreeSet;

use sim_conformance_evidence::{
    CheckedSubjectId, CheckerResultId, EvidenceGrade, RevocationStatus,
};
use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{EvidenceDecision, EvidenceReceiptId};

use super::{
    AuthorityTransfer, CheckerCatalog, CompletionGap, EvidenceCatalog, LinuxBoundary,
    RebuiltExecution, SelectedRoadmap,
};

pub(super) fn gaps(
    selected: &SelectedRoadmap,
    state: &RebuiltExecution,
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    transfers: &[AuthorityTransfer],
) -> Vec<CompletionGap> {
    let mut gaps = Vec::new();
    selection(selected, evidence, &mut gaps);
    subject(selected, state, evidence, &mut gaps);
    frontier(state, &mut gaps);
    artifacts(state, &mut gaps);
    leases(state, &mut gaps);
    releases(&evidence.releases, &mut gaps);
    transfer_gaps(evidence, transfers, &mut gaps);
    fresh(&evidence.fresh, &mut gaps);
    linux(evidence.linux, &mut gaps);
    claims(evidence, checks, &selected.final_world, &mut gaps);
    gaps
}

fn selection(
    selected: &SelectedRoadmap,
    evidence: &EvidenceCatalog,
    gaps: &mut Vec<CompletionGap>,
) {
    if selected.id != evidence.roadmap {
        gaps.push(gap(1, "roadmap-subject-mismatch"));
    }
    let selection = selected.human_selection.trim();
    let begin = selected.begin_authority.trim();
    if selection.is_empty() || begin.is_empty() || selection == begin {
        gaps.push(gap(1, "selection-begin"));
    }
    if selected.sibling_dependency.is_some() {
        gaps.push(gap(1, "normative-sibling"));
    }
    if selected.source_digest.trim().is_empty() || selected.source_digest != evidence.source_digest
    {
        gaps.push(gap(1, "source-digest"));
    }
}

fn subject(
    selected: &SelectedRoadmap,
    state: &RebuiltExecution,
    evidence: &EvidenceCatalog,
    gaps: &mut Vec<CompletionGap>,
) {
    if selected.final_world != evidence.final_world || selected.final_world != state.final_world {
        gaps.push(gap(11, "final-subject-mismatch"));
    }
    if evidence.replay_digest.trim().is_empty() || evidence.replay_digest != state.replay_digest {
        gaps.push(gap(17, "replay-digest"));
    }
}

fn frontier(state: &RebuiltExecution, gaps: &mut Vec<CompletionGap>) {
    if !state.frontier.is_empty() {
        gaps.push(gap(11, "frontier-open"));
    }
    if state.continuation.is_some() {
        gaps.push(gap(11, "continuation-open"));
    }
}

fn artifacts(state: &RebuiltExecution, gaps: &mut Vec<CompletionGap>) {
    if state.second_pass_work != 0 {
        gaps.push(gap(12, "second-pass-work"));
    }
    if state.artifacts.is_empty() {
        gaps.push(gap(12, "artifact-postimage"));
        return;
    }
    let mut owners = BTreeSet::new();
    let mut broken = false;
    for artifact in &state.artifacts {
        let owner = artifact.owner.trim();
        if owner.is_empty()
            || artifact.postimage.trim().is_empty()
            || !owners.insert(owner.to_owned())
        {
            broken = true;
        }
    }
    if broken {
        gaps.push(gap(12, "artifact-postimage"));
    }
}

fn leases(state: &RebuiltExecution, gaps: &mut Vec<CompletionGap>) {
    if !state.leases.is_empty() {
        gaps.push(gap(15, "live-lease"));
    }
    if !state.uncertain_effects.is_empty() {
        gaps.push(gap(15, "unresolved-effect"));
    }
}

fn releases(obligations: &[super::ReleaseObligation], gaps: &mut Vec<CompletionGap>) {
    if obligations.is_empty() {
        gaps.push(gap(21, "release-missing"));
        return;
    }
    let mut names = BTreeSet::new();
    let mut orders = Vec::new();
    for obligation in obligations {
        orders.push(obligation.order);
        let name = obligation.name.trim();
        if name.is_empty() {
            gaps.push(gap(21, "unpublished-git-source:missing-name"));
            continue;
        }
        if !names.insert(name.to_owned()) {
            gaps.push(gap(21, format!("release-duplicate:{name}")));
        }
        if !obligation.registry_published
            && !exact_git_source_release(name, obligation.source_release.trim())
        {
            gaps.push(gap(21, format!("unpublished-git-source:{name}")));
        }
    }
    let Ok(len) = u16::try_from(obligations.len()) else {
        gaps.push(gap(21, "release-order"));
        return;
    };
    orders.sort_unstable();
    let expected: Vec<u16> = (0..len).collect();
    if orders != expected {
        gaps.push(gap(21, "release-order"));
    }
}

/// `git+` plus 40 lowercase hex digits, the package name, and its version.
fn exact_git_source_release(name: &str, source: &str) -> bool {
    let Some(rest) = source.strip_prefix("git+") else {
        return false;
    };
    let Some((commit, spec)) = rest.split_once(':') else {
        return false;
    };
    if commit.len() != 40
        || !commit
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return false;
    }
    let Some((package, version)) = spec.split_once('@') else {
        return false;
    };
    package == name
        && !version.is_empty()
        && version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn transfer_gaps(
    evidence: &EvidenceCatalog,
    transfers: &[AuthorityTransfer],
    gaps: &mut Vec<CompletionGap>,
) {
    if transfers.is_empty() {
        gaps.push(gap(22, "transfer-missing"));
    }
    if evidence.model_provider_credential {
        gaps.push(gap(22, "provider-credential"));
    }
    for transfer in transfers {
        if transfer.id.value.trim().is_empty() {
            gaps.push(gap(22, "transfer-missing"));
        }
        if transfer.scope != "portable" {
            gaps.push(gap(22, "transfer-scope"));
        }
        if transfer.native_credential {
            gaps.push(gap(22, "native-credential"));
        }
        let role = transfer
            .retained_role
            .as_deref()
            .map(str::trim)
            .filter(|role| !role.is_empty());
        if !transfer.caller_zero && role.is_none() {
            gaps.push(gap(22, "transfer-role"));
        }
    }
}

fn linux(boundary: LinuxBoundary, gaps: &mut Vec<CompletionGap>) {
    if boundary == LinuxBoundary::SelectedUnfinished {
        gaps.push(gap(18, "linux-claim"));
    }
}

fn claims(
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    world: &super::WorldSealId,
    gaps: &mut Vec<CompletionGap>,
) {
    for row in &evidence.admissions {
        if !(1..=25).contains(&row.claim) {
            gaps.push(gap(row.claim, "claim-outside-selection"));
        }
    }
    for claim in 1..=25 {
        prove_claim(claim, evidence, checks, world, gaps);
    }
}

fn prove_claim(
    claim: u8,
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    world: &super::WorldSealId,
    gaps: &mut Vec<CompletionGap>,
) {
    let found: Vec<_> = evidence
        .admissions
        .iter()
        .filter(|row| row.claim == claim)
        .collect();
    if found.len() != 1 {
        gaps.push(gap(claim, "claim-unproved"));
        return;
    }
    if let Err(reason) = release_receipt(checks, world, found[0]) {
        gaps.push(gap(claim, reason));
    }
}

/// Receipt the catalog reuses at release grade for this bound admission.
///
/// Revocation comes from the owner map. A missing id is unknown, not current.
/// The checker receipt must name this claim, the selected subject, and the
/// catalog scope. The open evidence gate reports support checkers and then
/// the evidence record's own checker. That reuse is this checker's evidence
/// only when every consulted checker is the validated checker.
pub(super) fn release_receipt(
    checks: &CheckerCatalog,
    world: &super::WorldSealId,
    row: &super::ClaimAdmission,
) -> Result<EvidenceReceiptId, &'static str> {
    let status = checks
        .revocations
        .get(&row.checker)
        .cloned()
        .unwrap_or(RevocationStatus::Unknown);
    match status {
        RevocationStatus::Revoked => return Err("revoked-support"),
        RevocationStatus::Unknown => return Err("unknown-revocation"),
        RevocationStatus::Current => {}
    }
    let receipt = checks
        .catalog
        .checker_receipt(&row.checker, status)
        .map_err(|_| "evidence-not-verified")?;
    let subject = CheckedSubjectId::from_text(&world.value).map_err(|_| "evidence-not-verified")?;
    let result = CheckerResultId::from_text(&super::bound_result(row.claim))
        .map_err(|_| "evidence-not-verified")?;
    if receipt.subject() != &subject || receipt.scope() != &checks.scope {
        return Err("evidence-unrelated");
    }
    if receipt.result() != &result {
        return Err("claim-relabelled");
    }
    let mut consulted = Vec::new();
    let decision =
        checks
            .catalog
            .decide_evidence(&row.admitted.key, EvidenceGrade::Release, |id| {
                consulted.push(id.clone());
                checks
                    .revocations
                    .get(id)
                    .cloned()
                    .unwrap_or(RevocationStatus::Unknown)
            });
    match decision {
        EvidenceDecision::Reuse { receipt }
            if receipt == row.admitted.id
                && !consulted.is_empty()
                && consulted.iter().all(|id| id == &row.checker) =>
        {
            Ok(receipt)
        }
        EvidenceDecision::Refuse {
            reason: "revoked-support",
        } => Err("revoked-support"),
        EvidenceDecision::Refuse {
            reason: "unknown-revocation",
        } => Err("unknown-revocation"),
        _ => Err("evidence-not-verified"),
    }
}

fn fresh(record: &Datum, gaps: &mut Vec<CompletionGap>) {
    if let Some(reason) = gap_reason(record, "world/fresh-gap-v1") {
        let reason = if reason.is_empty() {
            "fresh-gap-unspecified".to_owned()
        } else {
            reason
        };
        gaps.push(gap(23, reason));
        return;
    }
    let Some(fields) = node_fields(record, "world/fresh-v1") else {
        gaps.push(gap(23, "fresh-record-unreadable"));
        return;
    };
    if string_field(fields, "seal").as_deref() != Some("unclaimed") {
        gaps.push(gap(23, "fresh-seal-not-evidence"));
        return;
    }
    if string_field(fields, "change").as_deref() == Some("shade-1") {
        gaps.push(gap(23, "construction-specimen"));
        return;
    }
    if string_field(fields, "succession").as_deref() != Some("portable") {
        gaps.push(gap(23, "fresh-succession"));
    }
    if string_field(fields, "linux").as_deref() != Some("absent") {
        gaps.push(gap(23, "linux-claim"));
    }
    if string_field(fields, "project")
        .unwrap_or_default()
        .trim()
        .is_empty()
        || string_field(fields, "change")
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        gaps.push(gap(23, "fresh-change"));
    }
    if [
        "converge-second",
        "deliver-second",
        "effects",
        "subprocesses",
    ]
    .into_iter()
    .any(|name| number_field(fields, name) != Some(0))
        || bool_field(fields, "resident") != Some(true)
    {
        gaps.push(gap(23, "fresh-second-pass"));
    }
    if record.canonical_bytes().is_err() {
        gaps.push(gap(23, "fresh-record-unreadable"));
    }
}

fn gap(claim: u8, reason: impl Into<String>) -> CompletionGap {
    CompletionGap {
        claim,
        reason: reason.into(),
    }
}

fn gap_reason(record: &Datum, tag: &str) -> Option<String> {
    string_field(node_fields(record, tag)?, "reason")
}

fn node_fields<'a>(record: &'a Datum, tag: &str) -> Option<&'a [(Symbol, Datum)]> {
    match record {
        Datum::Node { tag: found, fields } if found.as_qualified_str() == tag => Some(fields),
        _ => None,
    }
}

fn string_field(fields: &[(Symbol, Datum)], name: &str) -> Option<String> {
    match field(fields, name) {
        Some(Datum::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn number_field(fields: &[(Symbol, Datum)], name: &str) -> Option<u64> {
    match field(fields, name) {
        Some(Datum::Number(value)) => value.canonical.parse().ok(),
        _ => None,
    }
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> Option<bool> {
    match field(fields, name) {
        Some(Datum::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}
