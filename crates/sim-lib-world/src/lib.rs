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
//! merge is touched. A work case calls the typed packet receiver. Oversize
//! input and a wrong namespace, role, copy, or schema return before the
//! port is read. Incomplete output keeps its packet. Malformed retries stop
//! at the attempt ceiling. A proposal cannot carry an effect or an approval.
//! An authoring case calls the authoring compiler on one family. It lists,
//! shows, lints, selects, diffs, and compiles the selected source. A consulted
//! or superseded sibling does not satisfy a missing requirement. A missing
//! section, an authority decision, or a private value refuses.
//! One conduct call admits at most one effect or one pure transition.
//! A no-change wake starts nothing. A wait resumes once on a material
//! offer. A stale, wrong, replayed, or advisory direction refuses.
//! Reading the retained result does not conduct again.
//! One qualify call runs the published composition specimen once.
//! The record keeps one same-revision page, the exact lineage, a begin
//! that is separate from selection, and rejected advice. A Linux provider
//! and a changed closure refuse. Reading the retained result does not
//! qualify again.
//! One schedule call runs the published scheduler once. A later call
//! returns that record. A blocked queue does not block control. A wait
//! releases only that unit's leases. Stall and cleanup do not claim
//! process death.
//! One converge call runs the published revision once. A later call
//! returns that record. A stop before commit keeps the accepted
//! revision. A changed input opens only the claims that read it.
//! One fixed call runs the published fixed point once. A later call
//! returns that record. The second pass does no generation. A deletion
//! names that generator's closure. A failed proof or a repeated image
//! blocks the seal. The duplicate generator is not run.
//! One transact call runs the published model transaction once. A later
//! call returns that record. A second pass on the new head performs no
//! commit. Wrong head, dirt, private flow, a generated-owner bypass, a
//! stale base, and a region escape refuse. Overlap refuses. A partial
//! batch is not success. Rollback restores the pointer and keeps the
//! commit.
//! One land call runs the published landing once. A later call returns
//! that record. A foreign edit survives the landing. An already-true
//! replay does not move the branch. Rollback restores the exact commit.
//! A generated region refuses another actor. A cached answer stays
//! current until the head moves. Status names the tip and a missing
//! repository. The landing does not mutate a checkout.
//! One recover call runs the published recovery once. A later call
//! returns that record. An ambiguous observation requests no effect.
//! An unreconciled image is not resumed. Unresolved work and a changed
//! identity stop. A retryable failure admits one retry, and exhaustion
//! stops. Fresh lower-rank refinement is the only refiner request.
//! Image classification names unchanged, preimage, postimage, and
//! foreign bytes. A committed observation inverts to the prior image,
//! and a foreign observation refuses that inverse. Recovery does not
//! mutate a checkout.
//! One bind call runs the published pack binding once. A later call
//! returns that record. One binding serves two subjects and two scopes.
//! Swapped identities refuse. A missing pack scope is unimplemented.
//! The facet law passes, and its unfunded scope stays unimplemented.
//! Binding does not mutate a checkout.
//! One manifest call admits one owner table. A later call returns
//! that record. A public pin must be present and match the observed
//! head. A private pin, an escaped path, two revisions of one package,
//! two owners of one package, an unowned target, a dependency cycle,
//! an empty source, and a Linux row refuse. The record counts the
//! owners, packages, and edges that table admitted. Manifest admission
//! does not read a checkout.
//! One promote call admits one registry publication set. A later call
//! returns that record. A path source, a local receipt, a model plan,
//! a missing version, a missing checksum, a missing source identity,
//! a missing registry observation, a broken dependency order, a missing
//! install, an unreconciled effect, two rows of one package, an empty
//! set, and a Linux requirement refuse. The record counts the packages
//! and needs that set admitted. Promote admission does not contact a
//! registry.
//! One transfer call admits each portable responsibility move in one
//! document. A later call returns that record. Retirement requires
//! caller-zero evidence for the previous path and a zero caller count.
//! Cargo, Git, validation, and operator recovery stay under their named
//! roles. A native request, a publication segment, or a model credential
//! refuses before any effect. The record does not derive a completion seal.
//! One journal compare runs rebuild and the rebuild report on one
//! opened execution. Equal results are the same computation. A later
//! call returns that record. The compare does not close the frontier.
//! One operate call runs the operation lifecycle and the durable
//! operation service once. A verified dispatch is replayed without a
//! second performance. A missing acknowledgement stays dispatched. A
//! forged transition is refused. An interruption before or after an
//! admission recovers without repeating the dispatch. A later call
//! returns that record. The operation does not close the frontier.
//! A fresh succession reads one exported model record
//! and one later change. The export names registry packages, the foreign
//! project, and declared inputs. The change is observed, sealed, proved,
//! converged, landed, delivered, and recovered. The repeated convergence
//! and delivery do no further work. A warm checkout, a construction
//! fixture used as the change, or an unresolved hold stays open. Host
//! reboot, power loss, and protected production stay outside that record.
//! The loaded fresh command reads that export and one later change.
//! The construction specimen stays open when a succession record
//! closes. A warm checkout, a Linux claim, and an unresolved hold stay
//! open. The record does not derive a completion seal.
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
mod author;
mod bind;
mod canary;
mod command;
mod command_steps;
mod conduct;
mod converge;
mod deliver;
mod evidence;
mod fixed;
mod fixture;
mod fresh;
mod grant;
mod journal;
mod land;
mod manifest;
mod observe;
mod operate;
mod plan;
mod product;
mod promote;
mod prove;
mod prove_command;
mod provider;
mod qualify;
mod query;
mod recover;
mod release;
mod schedule;
mod seal;
mod seal_compile;
mod seal_workspace;
mod service;
mod succeed;
mod transact;
mod transfer;
mod work;

pub use command::{ModelDomainLoad, WORLD_VERB, WorldCommandLib};
pub use product::{
    DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, Declaration, SOURCE_CONCLUSION, SOURCE_FACT,
    WorldError, WorldProduct, WorldProjection,
};
pub use prove_command::{PROVE_VERB, ProveCommandLib};
pub use seal::{captured_inputs, workspace_root};

#[cfg(test)]
#[path = "../build/activate.rs"]
mod activate_rules;
#[cfg(test)]
mod canary_tests;
#[cfg(test)]
#[path = "../build/closure_flag.rs"]
mod closure_flag;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod evidence_tests;
#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod observe_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod prove_argv_tests;
#[cfg(test)]
mod prove_tests;
#[cfg(test)]
mod prove_union_tests;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod seal_compile_tests;
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
#[cfg(test)]
mod transfer_tests;
