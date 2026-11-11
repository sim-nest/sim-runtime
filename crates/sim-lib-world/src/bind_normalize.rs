// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Canonical identity for one bound subject.
//!
//! The identity is the kernel content id of the subject node. Field order
//! is not part of that id. A diagnostic is retained beside the node and is
//! not hashed. A role or field change changes the id. A wrong role, an old
//! algorithm, or a digest that was not computed from the node fails here.

use sim_kernel::{ContentId, Datum, Symbol, datum_content_algorithm};

use crate::WorldError;

const ROLES: &[&str] = &["member", "owner"];
const BASE: &[(&str, &str)] = &[("name", "alpha"), ("place", "one")];
const REORDERED: &[(&str, &str)] = &[("place", "one"), ("name", "alpha")];
const RENAMED: &[(&str, &str)] = &[("name", "beta"), ("place", "one")];

pub(crate) struct Normalized {
    pub(crate) reason: &'static str,
    pub(crate) alias: &'static str,
    pub(crate) diagnostic: &'static str,
    pub(crate) field: &'static str,
    pub(crate) role: &'static str,
    pub(crate) wrong_role: &'static str,
    pub(crate) old_byte: &'static str,
    pub(crate) authority: &'static str,
}

pub(crate) struct Bound {
    node: Datum,
    diagnostic: String,
}

/// Compares one subject with its alias, diagnostic, field, and role changes.
///
/// # Errors
/// Returns an error when a well-formed subject has no kernel content id.
pub(crate) fn survey() -> Result<Normalized, WorldError> {
    let base = bind_subject("member", BASE, "note-a")?;
    let reordered = bind_subject("member", REORDERED, "note-a")?;
    let diagnosed = bind_subject("member", BASE, "note-b")?;
    let renamed = bind_subject("member", RENAMED, "note-a")?;
    let owner = bind_subject("owner", BASE, "note-a")?;
    let base_id = identity(&base)?;
    let alias = mark(identity(&reordered)? == base_id);
    let diagnostic =
        mark(identity(&diagnosed)? == base_id && diagnosed.diagnostic != base.diagnostic);
    let field = mark(identity(&renamed)? == base_id);
    let role = mark(identity(&owner)? == base_id);
    let wrong_role = refusal(bind_subject("guest", BASE, "note-a").is_err());
    let old = ContentId::from_bytes(Symbol::qualified("core", "sha256"), base_id.bytes);
    let mut forged_bytes = base_id.bytes;
    forged_bytes[0] ^= 0xff;
    let forged = ContentId::from_bytes(datum_content_algorithm(), forged_bytes);
    let old_byte = refusal(accept(&base, &old).is_err());
    let authority = refusal(accept(&base, &forged).is_err());
    let matched = accept(&base, &base_id).is_ok();
    let reason = if alias == "same"
        && diagnostic == "same"
        && field == "moved"
        && role == "moved"
        && wrong_role == "refused"
        && old_byte == "refused"
        && authority == "refused"
        && matched
    {
        "normalized"
    } else {
        "drifted"
    };
    Ok(Normalized {
        reason,
        alias,
        diagnostic,
        field,
        role,
        wrong_role,
        old_byte,
        authority,
    })
}

pub(crate) fn bind_subject(
    role: &str,
    fields: &[(&str, &str)],
    diagnostic: &str,
) -> Result<Bound, WorldError> {
    Ok(Bound {
        node: node(role, fields)?,
        diagnostic: diagnostic.to_owned(),
    })
}

pub(crate) fn identity(bound: &Bound) -> Result<ContentId, WorldError> {
    bound.node.content_id().map_err(broke)
}

pub(crate) fn accept(bound: &Bound, presented: &ContentId) -> Result<ContentId, WorldError> {
    let id = identity(bound)?;
    if presented.algorithm != datum_content_algorithm() {
        return Err(WorldError::Qualification("old-byte".to_owned()));
    }
    if presented.bytes != id.bytes {
        return Err(WorldError::Qualification("fabricated-authority".to_owned()));
    }
    Ok(id)
}

fn node(role: &str, fields: &[(&str, &str)]) -> Result<Datum, WorldError> {
    if !ROLES.contains(&role) {
        return Err(WorldError::Qualification(format!("wrong-role {role}")));
    }
    let mut pairs = vec![(
        Symbol::new("role"),
        Datum::Symbol(Symbol::qualified("world", role)),
    )];
    for (name, value) in fields {
        pairs.push((Symbol::new(*name), Datum::String((*value).to_owned())));
    }
    Ok(Datum::Node {
        tag: Symbol::qualified("world", "bound-subject-v1"),
        fields: pairs,
    })
}

fn mark(equal: bool) -> &'static str {
    if equal { "same" } else { "moved" }
}

fn refusal(failed: bool) -> &'static str {
    if failed { "refused" } else { "accepted" }
}

fn broke(error: sim_kernel::Error) -> WorldError {
    WorldError::Qualification(error.to_string())
}

#[cfg(test)]
#[path = "bind_normalize_tests.rs"]
mod tests;
