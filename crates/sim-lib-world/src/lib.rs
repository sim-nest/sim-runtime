// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loadable read-only world projection, explanation, and planning product.
//!
//! The product receives already observed semantic facts from its caller. It can
//! project, compare, explain, and plan those facts. Projection, diff, why, and
//! plan start no observation and no process. Explicit snapshot and
//! snapshot-diff read one sealed model batch through the domain port named by
//! the selector. Semantic identity binds freshness, provenance, and policy and
//! excludes diagnostic envelopes. A native realization returns unavailable and
//! does not read the model batch or a host. Service observation reads exact
//! branch, tag, direct, and peeled targets, package checksums, and CI and
//! mirror facts from a named model port. A plan omits a current match, retains
//! a divergent or unknown fact, and does not mutate the reading. A native or
//! real-service request does not consult those model facts. Provenance remains
//! when the reading is encoded and decoded. A caller-named batch carries its
//! own provider rows, nested resolver roots, generated input, and proof
//! declarations through the same commands. An undeclared provider returns a
//! gap. A model remote stays unavailable and is not contacted. Seal and
//! universe enumerate the resolver inputs captured for this crate, admit only
//! verified inputs, and refuse a missing, corrupt, wrong-target, or stub-only
//! compiling input before any compiler opens. The loaded command reports that custody, refuses
//! an unsupported pair or a malformed call, measures resident cached why from
//! one sealed snapshot, reports projection sensitivity from provider-read
//! baseline and variant bytes, and returns a deterministic proof plan for one
//! sealed in-memory subject. The prove command executes every selected pure
//! check on that sealed subject with reuse disabled, then compares a reuse
//! decision with those cold digests.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod assay;
mod assay_mutant;
mod command;
mod fixture;
mod grant;
mod observe;
mod plan;
mod product;
mod prove;
mod prove_command;
mod provider;
mod query;
mod seal;
mod service;

pub use command::{ModelDomainLoad, WORLD_VERB, WorldCommandLib};
pub use product::{
    DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, SOURCE_CONCLUSION, SOURCE_FACT, WorldError,
    WorldProduct, WorldProjection,
};
pub use prove_command::{PROVE_VERB, ProveCommandLib};

#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod observe_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod prove_tests;
#[cfg(test)]
mod seal_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
