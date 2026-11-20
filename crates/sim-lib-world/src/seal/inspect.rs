// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsed face of one sealed source document.
//!
//! The face is the real parser and the real seal. A document that never
//! reaches those steps stays an early refusal.

use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::Datum;

use crate::WorldError;

pub(crate) struct FinalFace {
    pub(crate) early: Option<&'static str>,
    pub(crate) admission: String,
    pub(crate) refusal: String,
    pub(crate) target: String,
    pub(crate) compiling_domains: usize,
    pub(crate) verified_checksums: usize,
    pub(crate) features: usize,
    pub(crate) config_resolver: bool,
    pub(crate) config_lock: bool,
    pub(crate) toolchain_release: String,
    pub(crate) nested: bool,
    pub(crate) marks: BTreeMap<String, String>,
    pub(crate) policies: BTreeMap<String, String>,
    pub(crate) reads_facts: BTreeSet<String>,
    pub(crate) reads_policies: BTreeSet<String>,
}

impl FinalFace {
    fn early(reason: &'static str) -> Self {
        Self {
            early: Some(reason),
            admission: String::new(),
            refusal: String::new(),
            target: String::new(),
            compiling_domains: 0,
            verified_checksums: 0,
            features: 0,
            config_resolver: false,
            config_lock: false,
            toolchain_release: String::new(),
            nested: false,
            marks: BTreeMap::new(),
            policies: BTreeMap::new(),
            reads_facts: BTreeSet::new(),
            reads_policies: BTreeSet::new(),
        }
    }
}

pub(crate) fn inspect_final(input: Option<&str>) -> Result<FinalFace, WorldError> {
    let owned;
    let text = match input {
        None => super::batch::captured(),
        Some(input) => {
            owned = super::batch::document_text(input)
                .ok_or_else(|| WorldError::Qualification("unknown-batch".to_owned()))?;
            owned.as_str()
        }
    };
    let inputs = match super::parse::parse(text) {
        Ok(inputs) => inputs,
        Err(WorldError::Qualification(message))
            if message.contains("source input is incomplete")
                && !text.lines().any(|line| line.starts_with("sealed-target ")) =>
        {
            return Ok(FinalFace::early("missing-target"));
        }
        Err(WorldError::Qualification(message))
            if message.contains("unknown domain")
                && !text.lines().any(|line| line.starts_with("domain ")) =>
        {
            return Ok(FinalFace::early("missing-resolver"));
        }
        Err(error) => return Err(error),
    };
    let assessed = super::assess_text("admitted", text, None)?;
    let admission = if assessed.gap {
        "gap".to_owned()
    } else {
        string_field(&assessed.seal, "admission").unwrap_or_default()
    };
    let refusal = string_field(&assessed.seal, "reason").unwrap_or_default();
    Ok(FinalFace {
        early: None,
        admission,
        refusal,
        target: inputs.sealed_target,
        compiling_domains: inputs
            .domains
            .values()
            .filter(|domain| domain.role == "compiling")
            .count(),
        verified_checksums: inputs
            .members
            .iter()
            .filter(|member| {
                member.closure
                    && member.kind != "none"
                    && member.checksum.len() == 64
                    && member.checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .count(),
        features: inputs.features.len(),
        config_resolver: inputs
            .config
            .get("resolver")
            .is_some_and(|value| !value.is_empty()),
        config_lock: inputs
            .config
            .get("lock")
            .is_some_and(|value| !value.is_empty()),
        toolchain_release: inputs.toolchain.get("release").cloned().unwrap_or_default(),
        nested: !inputs.generated.is_empty()
            || inputs.members.iter().any(|member| !member.deps.is_empty()),
        marks: assessed.marks,
        policies: inputs.policies,
        reads_facts: inputs.reads_facts,
        reads_policies: inputs.reads_policies,
    })
}

fn string_field(value: &Datum, name: &str) -> Option<String> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    fields.iter().find_map(|(field, item)| {
        if field.as_qualified_str() != name {
            return None;
        }
        match item {
            Datum::String(text) => Some(text.clone()),
            _ => None,
        }
    })
}
