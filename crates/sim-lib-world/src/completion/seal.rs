// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Seal assembly. The digest is a pure function of the proving inputs.

use sha2::{Digest, Sha256};

use super::{
    AuthorityTransfer, CheckerCatalog, CheckerReceiptId, CompletionClaim, CompletionSeal,
    CompletionSealId, EvidenceCatalog, LinuxBoundary, RebuiltExecution, SealDomain,
    SelectedRoadmap,
};

pub(super) fn from_proved(
    selected: &SelectedRoadmap,
    state: &RebuiltExecution,
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    transfers: &[AuthorityTransfer],
) -> CompletionSeal {
    let mut claims = Vec::with_capacity(25);
    let mut checker_receipts = Vec::with_capacity(25);
    for number in 1..=25 {
        let row = evidence.admissions.iter().find(|row| row.claim == number);
        let key = row
            .map(|row| id_hex(row.admitted.key.content_id().bytes))
            .unwrap_or_default();
        if let Some(row) = row
            && let Ok(receipt) = super::check::release_receipt(checks, &selected.final_world, row)
        {
            checker_receipts.push(CheckerReceiptId {
                value: id_hex(receipt.content_id().bytes),
            });
        }
        claims.push(CompletionClaim { number, key });
    }
    checker_receipts.sort();
    let mut authority_transfers: Vec<_> = transfers
        .iter()
        .map(|transfer| transfer.id.clone())
        .collect();
    authority_transfers.sort();
    CompletionSeal {
        roadmap: selected.id.clone(),
        final_world: selected.final_world.clone(),
        claims,
        evidence: evidence.id.clone(),
        checker_receipts,
        authority_transfers,
        digest: CompletionSealId {
            value: digest(selected, state, evidence, checks, transfers),
        },
        domain: SealDomain::PortableClosed,
    }
}

fn digest(
    selected: &SelectedRoadmap,
    state: &RebuiltExecution,
    evidence: &EvidenceCatalog,
    checks: &CheckerCatalog,
    transfers: &[AuthorityTransfer],
) -> String {
    let mut text = String::new();
    push(&mut text, "roadmap", &selected.id.value);
    push(&mut text, "world", &selected.final_world.value);
    push(&mut text, "selection", &selected.human_selection);
    push(&mut text, "begin", &selected.begin_authority);
    push(&mut text, "source", &selected.source_digest);
    push(
        &mut text,
        "sibling",
        selected.sibling_dependency.as_deref().unwrap_or(""),
    );
    push(&mut text, "replay", &state.replay_digest);
    push(&mut text, "frontier", &state.frontier.join(","));
    push(
        &mut text,
        "continuation",
        state.continuation.as_deref().unwrap_or(""),
    );
    push(&mut text, "second", &state.second_pass_work.to_string());
    push(&mut text, "leases", &state.leases.join(","));
    push(&mut text, "uncertain", &state.uncertain_effects.join(","));
    for artifact in &state.artifacts {
        push(
            &mut text,
            "artifact",
            &format!("{}={}", artifact.owner, artifact.postimage),
        );
    }
    push(&mut text, "evidence", &evidence.id.value);
    push(&mut text, "evidence-roadmap", &evidence.roadmap.value);
    push(&mut text, "evidence-world", &evidence.final_world.value);
    push(&mut text, "evidence-source", &evidence.source_digest);
    push(&mut text, "evidence-replay", &evidence.replay_digest);
    let mut admissions = evidence.admissions.clone();
    admissions.sort_by_key(|row| row.claim);
    for row in admissions {
        let receipt = super::check::release_receipt(checks, &selected.final_world, &row)
            .map(|receipt| id_hex(receipt.content_id().bytes))
            .unwrap_or_default();
        push(
            &mut text,
            "admission",
            &format!(
                "{}:{}:{}",
                row.claim,
                id_hex(row.admitted.key.content_id().bytes),
                receipt
            ),
        );
    }
    let fresh = evidence.fresh.canonical_bytes().unwrap_or_default();
    push(&mut text, "fresh", &hex(&fresh));
    for release in &evidence.releases {
        push(
            &mut text,
            "release",
            &format!(
                "{}:{}:{}:{}",
                release.order, release.name, release.registry_published, release.source_release
            ),
        );
    }
    push(&mut text, "linux", linux_token(evidence.linux));
    push(
        &mut text,
        "provider-credential",
        if evidence.model_provider_credential {
            "yes"
        } else {
            "no"
        },
    );
    for transfer in transfers {
        let role = transfer.retained_role.as_deref().unwrap_or("");
        push(
            &mut text,
            "transfer",
            &format!(
                "{}:{}:{}:{}:{}",
                transfer.id.value,
                transfer.scope,
                transfer.caller_zero,
                role,
                transfer.native_credential
            ),
        );
    }
    hex(Sha256::digest(text.as_bytes()).as_slice())
}

fn linux_token(boundary: LinuxBoundary) -> &'static str {
    match boundary {
        LinuxBoundary::Unselected => "unselected",
        LinuxBoundary::Absent => "absent",
        LinuxBoundary::SelectedUnfinished => "selected-unfinished",
    }
}

fn push(text: &mut String, name: &str, value: &str) {
    text.push_str(name);
    text.push('=');
    text.push_str(value);
    text.push('\n');
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn id_hex(bytes: [u8; 32]) -> String {
    hex(&bytes)
}
