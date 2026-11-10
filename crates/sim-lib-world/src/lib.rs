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
//! when the reading is encoded and decoded. A release plan names each remaining
//! reconciliation, its reason, and the audit disclosure. It omits a completed
//! fact, keeps an uncertain original, and refuses a private value. Status and
//! why read that plan. A plan with nothing remaining causes no effect. A
//! delivery qualification runs that plan through the durable operation
//! lifecycle against the bound model service. It omits performance for a
//! completed fact, blocks on a divergent value or a wrong tag, and keeps a
//! lagging registry, stale observation, denied reading, partial result,
//! uncertain reading, or unavailable receiver uncertain. Recovery observes
//! before any further performance. The model sink publishes nothing. Delivery
//! status and why read an admitted revision-fenced view of that qualification.
//! A missing or stale view asks for a refresh and does not run the lifecycle
//! again. A model delivery canary places one validated source head onto a fresh
//! model branch under a separate purpose grant. It runs once, recovers,
//! and repeats the reconciliation with no further performance. A wrong or
//! consumed purpose grant refuses. No public branch, tag, package, or
//! merge is touched. A fresh succession reads one exported model record
//! and one later change. The export names registry packages, the foreign
//! project, and declared inputs. The change is observed, sealed, proved,
//! converged, landed, delivered, and recovered. The repeated convergence
//! and delivery do no further work. A warm checkout, a construction
//! fixture used as the change, or an unresolved hold stays open. Host
//! reboot, power loss, and protected production stay outside that record.
//! An admitted observation document carries provider rows.
//! An admitted input document carries nested resolver roots and generated
//! input. Bundled proof declarations stay on the loaded product. An
//! undeclared provider returns a gap. A model remote stays unavailable and
//! is not contacted. Seal and
//! universe enumerate the resolver inputs captured for this crate, admit only
//! verified inputs, and refuse a missing, corrupt, wrong-target, or stub-only
//! compiling input before any compiler opens. The loaded command reports that custody, refuses
//! an unsupported pair or a malformed call, measures resident cached why from
//! one sealed snapshot, reports projection sensitivity from provider-read
//! baseline and variant bytes, and returns a deterministic proof plan for one
//! sealed in-memory subject. The prove command's cold denominator is the
//! portable phase gates. A gate with a reachable owner result is one leaf of
//! the roadmap proof catalog. A gate without one stays a named gap, and the
//! frontier stays open. The sensitivity assay is a separate conclusion set.
//! A pure leaf is an artifact comparison or a source-deck predicate. A
//! command leaf is launched on the qualified route. Each leaf digest binds the
//! verdict and the retained capture. The admitted view keeps those capture
//! bytes. The subject binds the proof implementation. A scripted
//! command receipt is not a proof. A later
//! reuse, status, why, or frontier query reads
//! that admitted view. A missing or stale view asks for a refresh and does
//! not execute the cold path again.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod assay;
mod assay_mutant;
mod assay_oracle;
mod canary;
mod command;
mod deliver;
mod fixture;
mod grant;
mod observe;
mod plan;
mod product;
mod prove;
mod prove_command;
mod provider;
mod query;
mod release;
mod seal;
mod service;
mod succeed;

pub use command::{ModelDomainLoad, WORLD_VERB, WorldCommandLib};
pub use product::{
    DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, Declaration, SOURCE_CONCLUSION, SOURCE_FACT,
    WorldError, WorldProduct, WorldProjection,
};
pub use prove_command::{PROVE_VERB, ProveCommandLib};
pub use seal::{captured_inputs, workspace_root};

#[cfg(test)]
mod canary_tests;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod observe_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod prove_tests;
#[cfg(test)]
mod prove_union_tests;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod seal_source_tests;
#[cfg(test)]
mod seal_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod succeed_tests;
#[cfg(test)]
mod tests;
