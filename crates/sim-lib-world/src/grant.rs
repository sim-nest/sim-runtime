// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Live currentness for one model-domain load.
//!
//! The grant is the checker owner for that load. Public use calls
//! [`ModelDomainGrant::admit`], which re-reads
//! [`sim_conformance_core::LiveCheckerReceipt::verify_current`]. Revocation
//! and owner teardown are the two ways that check refuses.

use std::collections::BTreeSet;

use sim_conformance_core::{
    CheckArgument, CheckInputClosureId, CheckInvocation, CheckScopeId, CheckTemplate,
    CheckedSubjectId, CheckerBinding, CheckerResultId, CommandId, ConformancePackId,
    EnvironmentPolicyId, EvidenceGrade, EvidenceProvenanceId, EvidenceSetId, LiveCheckerAuthority,
    LiveCheckerOwner, LiveCheckerReceipt, OutputShapeId, OwnerBindingId, PolicyId, ProofCodeId,
    RevocationSourceId, WorkingDirectoryPolicyId,
};
use sim_kernel::{Datum, Symbol};

use crate::{ModelDomainLoad, WorldError};

/// One generation of a model-domain load grant.
pub(crate) struct ModelDomainGrant {
    load: ModelDomainLoad,
    owner: Option<LiveCheckerOwner>,
    authority: LiveCheckerAuthority,
    receipt: LiveCheckerReceipt,
    invocation: CheckInvocation,
}

impl ModelDomainGrant {
    /// Boots an owner, marks this load current, and issues its only receipt.
    pub(crate) fn issue(load: ModelDomainLoad) -> Result<Self, WorldError> {
        let scope = CheckScopeId::from_text("world/model-domain-use-v1").map_err(qualify)?;
        let binding = binding(scope.clone())?;
        let subject = load_subject(&load)?;
        let invocation = binding
            .instantiate(
                ProofCodeId::from_text("sim_lib_world::grant::issue").map_err(qualify)?,
                ConformancePackId::from_text("world/model-domain-v1").map_err(qualify)?,
                subject,
                scope,
                load_closure(&load)?,
            )
            .map_err(qualify)?;
        let generation = Datum::String(format!(
            "world/model-domain-load/{}/{}/{}",
            load.provider, load.loader, load.artifact
        ))
        .content_id()
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
        let (owner, issuer) = LiveCheckerOwner::boot(
            generation,
            binding,
            PolicyId::from_text("world/model-domain-policy-v1").map_err(qualify)?,
        );
        owner.mark_current(&invocation).map_err(qualify)?;
        let authority = owner.authority();
        let receipt = issuer
            .issue(
                invocation.clone(),
                CheckerResultId::from_text("world/model-domain-passed-v1").map_err(qualify)?,
                EvidenceGrade::Bootstrap,
                EvidenceProvenanceId::from_text("world/model-domain-provenance-v1")
                    .map_err(qualify)?,
                EvidenceSetId::from_text("world/model-domain-support-v1").map_err(qualify)?,
            )
            .map_err(qualify)?;
        Ok(Self {
            load,
            owner: Some(owner),
            authority,
            receipt,
            invocation,
        })
    }

    /// The load this grant authorizes.
    pub(crate) fn load(&self) -> &ModelDomainLoad {
        &self.load
    }

    /// Refuses unless this receipt is still current for its live owner.
    pub(crate) fn admit(&self) -> Result<(), WorldError> {
        self.receipt
            .verify_current(&self.authority)
            .map(|_| ())
            .map_err(qualify)
    }

    /// Selects `Revoked` for this grant's exact invocation.
    pub(crate) fn revoke(&self) -> Result<(), WorldError> {
        let owner = self.owner.as_ref().ok_or_else(owner_not_live)?;
        owner.revoke(&self.invocation).map_err(qualify)
    }

    /// Drops the owner. Later admission fails because the owner is not live.
    pub(crate) fn teardown(&mut self) {
        self.owner.take();
    }

    /// Replaces this generation with a new current grant for the same load.
    pub(crate) fn reopen(&mut self) -> Result<(), WorldError> {
        *self = Self::issue(self.load.clone())?;
        Ok(())
    }
}

/// Text of a qualification failure, without the `WorldError` wrapper.
pub(crate) fn refusal_reason(error: &WorldError) -> String {
    match error {
        WorldError::Qualification(text) => text.clone(),
        other => other.to_string(),
    }
}

fn owner_not_live() -> WorldError {
    WorldError::Qualification("checker owner is not live".to_owned())
}

fn qualify(error: impl std::fmt::Display) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn binding(scope: CheckScopeId) -> Result<CheckerBinding, WorldError> {
    let template = CheckTemplate::new(
        "sim_lib_world::grant::issue".to_owned(),
        vec![
            CheckArgument::BindingSlot,
            CheckArgument::SubjectSlot,
            CheckArgument::ScopeSlot,
        ],
        WorkingDirectoryPolicyId::from_text("world/model-domain-cwd-v1").map_err(qualify)?,
        EnvironmentPolicyId::from_text("world/model-domain-env-v1").map_err(qualify)?,
        OutputShapeId::from_text("world/model-domain-result-v1").map_err(qualify)?,
    )
    .map_err(qualify)?;
    CheckerBinding::new(
        "world/model-domain-load".to_owned(),
        OwnerBindingId::from_text("platform/site/model").map_err(qualify)?,
        "cli/main/world".to_owned(),
        vec![ConformancePackId::from_text("world/model-domain-v1").map_err(qualify)?],
        OutputShapeId::from_text("world/model-domain-receipt-v1").map_err(qualify)?,
        RevocationSourceId::from_text("world/model-domain-revocation-v1").map_err(qualify)?,
        CommandId::from_text("world/model-domain-validation-v1").map_err(qualify)?,
        CommandId::from_text("world/model-domain-docs-v1").map_err(qualify)?,
        BTreeSet::from([scope]),
        template,
    )
    .map_err(qualify)
}

fn load_fields(load: &ModelDomainLoad) -> Vec<(Symbol, Datum)> {
    vec![
        (
            Symbol::new("provider"),
            Datum::String(load.provider.clone()),
        ),
        (Symbol::new("loader"), Datum::String(load.loader.clone())),
        (
            Symbol::new("artifact"),
            Datum::String(load.artifact.clone()),
        ),
    ]
}

fn load_subject(load: &ModelDomainLoad) -> Result<CheckedSubjectId, WorldError> {
    CheckedSubjectId::from_fields(load_fields(load)).map_err(qualify)
}

fn load_closure(load: &ModelDomainLoad) -> Result<CheckInputClosureId, WorldError> {
    CheckInputClosureId::from_fields(load_fields(load)).map_err(qualify)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModelDomainLoad {
        ModelDomainLoad {
            provider: "platform/site/model".to_owned(),
            loader: "loader/static-v1".to_owned(),
            artifact: "lib/world-command".to_owned(),
        }
    }

    #[test]
    fn revoke_and_dropped_owner_refuse_before_a_successor_admits() {
        let grant = ModelDomainGrant::issue(sample()).unwrap();
        grant.admit().unwrap();
        grant.revoke().unwrap();
        let revoked = grant.admit().unwrap_err();
        assert_eq!(refusal_reason(&revoked), "RevocationUnknownOrActive");

        let mut grant = ModelDomainGrant::issue(sample()).unwrap();
        grant.teardown();
        let stale = grant.admit().unwrap_err();
        assert_eq!(refusal_reason(&stale), "checker owner is not live");
        grant.reopen().unwrap();
        grant.admit().unwrap();
    }
}
