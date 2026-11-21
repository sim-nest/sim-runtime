// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded work command collected for one subject.
//!
//! Each case is the existing work command. Oversize input returns before
//! the port is read. Incomplete output keeps the packet. Malformed retries
//! stop at the attempt ceiling. A retry that does not narrow refuses. A
//! wrong namespace, role, copy, or schema refuses when the mirrors agree.
//! A model proposal cannot carry an effect or an approval. A private value
//! is refused before the reservation and is not copied. This module does
//! not derive a completion seal.

use sha2::{Digest, Sha256};

use crate::WorldError;

#[path = "work_execute_parse.rs"]
mod parse;

const ZERO_SEAL: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Reasons the loaded work command returns for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalWork {
    /// The admitted packet is present and carries no effect.
    pub admitted: String,
    /// Oversize input is refined before the port is read.
    pub oversize: String,
    /// Incomplete output keeps the checked partial packet.
    pub incomplete: String,
    /// Malformed output stops at the attempt ceiling.
    pub malformed: String,
    /// A retry that does not narrow is refused.
    pub descent: String,
    /// A wrong namespace is refused before the port is read.
    pub namespace: String,
    /// A wrong role is refused before the port is read.
    pub role: String,
    /// A stale copy is refused before the port is read.
    pub stale: String,
    /// A wrong schema is refused before the port is read.
    pub schema: String,
    /// A model proposal cannot carry an effect or an approval.
    pub effects: String,
    /// A private value is refused before the reservation.
    pub leakage: String,
    /// Digest of the admitted command record.
    pub admitted_seal: String,
    /// Digest of the oversize command record.
    pub oversize_seal: String,
    /// Digest of the incomplete command record.
    pub incomplete_seal: String,
    /// Digest of the malformed command record.
    pub malformed_seal: String,
    /// Digest of the descent command record.
    pub descent_seal: String,
    /// Digest of the namespace command record.
    pub namespace_seal: String,
    /// Digest of the role command record.
    pub role_seal: String,
    /// Digest of the stale command record.
    pub stale_seal: String,
    /// Digest of the schema command record.
    pub schema_seal: String,
    /// Digest of the effects command record.
    pub effects_seal: String,
    /// Digest of the leakage command record.
    pub leakage_seal: String,
}

/// Runs the selected work cases through the loaded work command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a record
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalWork, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (admitted, admitted_seal) = sealed_work(
        &command,
        "admitted",
        &WorkLaw {
            class: "admitted",
            packet: "present",
            partial: "",
            proposal: "mapped",
            reads: "1",
            attempts: "1",
            bytes: "12",
            files: "1",
            tokens: "12",
            workers: "1",
            proposals: "1",
        },
    )?;
    let (oversize, oversize_seal) = sealed_gap(
        &command,
        "oversize",
        &GapLaw {
            case: "oversize",
            reason: "oversize",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "",
            workers: "0",
            proposals: "0",
        },
    )?;
    let (incomplete, incomplete_seal) = sealed_work(
        &command,
        "incomplete",
        &WorkLaw {
            class: "incomplete",
            packet: "present",
            partial: "partial-map",
            proposal: "",
            reads: "1",
            attempts: "1",
            bytes: "12",
            files: "1",
            tokens: "12",
            workers: "1",
            proposals: "1",
        },
    )?;
    let (malformed, malformed_seal) = sealed_gap(
        &command,
        "malformed",
        &GapLaw {
            case: "malformed",
            reason: "malformed-bound",
            reads: "1",
            attempts: "2",
            evidence: "second",
            mirrors: "",
            workers: "2",
            proposals: "2",
        },
    )?;
    let (descent, descent_seal) = sealed_gap(
        &command,
        "descent",
        &GapLaw {
            case: "descent",
            reason: "no-descent",
            reads: "1",
            attempts: "0",
            evidence: "same",
            mirrors: "",
            workers: "1",
            proposals: "1",
        },
    )?;
    let (namespace, namespace_seal) = sealed_gap(
        &command,
        "namespace",
        &GapLaw {
            case: "namespace",
            reason: "namespace",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "true",
            workers: "0",
            proposals: "0",
        },
    )?;
    let (role, role_seal) = sealed_gap(
        &command,
        "role",
        &GapLaw {
            case: "role",
            reason: "role",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "true",
            workers: "0",
            proposals: "0",
        },
    )?;
    let (stale, stale_seal) = sealed_gap(
        &command,
        "stale",
        &GapLaw {
            case: "stale",
            reason: "stale-copy",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "true",
            workers: "0",
            proposals: "0",
        },
    )?;
    let (schema, schema_seal) = sealed_gap(
        &command,
        "schema",
        &GapLaw {
            case: "schema",
            reason: "schema",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "true",
            workers: "0",
            proposals: "0",
        },
    )?;
    let (effects, effects_seal) = sealed_gap(
        &command,
        "effects",
        &GapLaw {
            case: "effects",
            reason: "effects-or-approval",
            reads: "1",
            attempts: "0",
            evidence: "",
            mirrors: "",
            workers: "1",
            proposals: "1",
        },
    )?;
    let (leakage, leakage_seal) = sealed_gap(
        &command,
        parse::MARKER,
        &GapLaw {
            case: "",
            reason: "private-leakage",
            reads: "0",
            attempts: "0",
            evidence: "",
            mirrors: "",
            workers: "0",
            proposals: "0",
        },
    )?;
    let seals = [
        admitted_seal.as_str(),
        oversize_seal.as_str(),
        incomplete_seal.as_str(),
        malformed_seal.as_str(),
        descent_seal.as_str(),
        namespace_seal.as_str(),
        role_seal.as_str(),
        stale_seal.as_str(),
        schema_seal.as_str(),
        effects_seal.as_str(),
        leakage_seal.as_str(),
    ];
    let mut unique = std::collections::BTreeSet::new();
    if seals
        .iter()
        .any(|seal| !hex_seal(seal) || !unique.insert(*seal))
    {
        return Err(parse::fault("seal-identity"));
    }
    Ok(FinalWork {
        admitted,
        oversize,
        incomplete,
        malformed,
        descent,
        namespace,
        role,
        stale,
        schema,
        effects,
        leakage,
        admitted_seal,
        oversize_seal,
        incomplete_seal,
        malformed_seal,
        descent_seal,
        namespace_seal,
        role_seal,
        stale_seal,
        schema_seal,
        effects_seal,
        leakage_seal,
    })
}

struct WorkLaw {
    class: &'static str,
    packet: &'static str,
    partial: &'static str,
    proposal: &'static str,
    reads: &'static str,
    attempts: &'static str,
    bytes: &'static str,
    files: &'static str,
    tokens: &'static str,
    workers: &'static str,
    proposals: &'static str,
}

struct GapLaw {
    case: &'static str,
    reason: &'static str,
    reads: &'static str,
    attempts: &'static str,
    evidence: &'static str,
    mirrors: &'static str,
    workers: &'static str,
    proposals: &'static str,
}

fn sealed_work(
    command: &crate::command::WorldCommandLib,
    case: &str,
    law: &WorkLaw,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&argv(case))?;
    parse::refuse_copied(&datum)?;
    if parse::kind_of(&datum) != "world/work-v1" {
        return Err(parse::fault("work-record"));
    }
    require(&parse::text(&datum, "case")?, case, "case")?;
    require(&parse::text(&datum, "class")?, law.class, "class")?;
    require(&parse::text(&datum, "packet")?, law.packet, "packet")?;
    require(&parse::text(&datum, "partial")?, law.partial, "partial")?;
    require(&parse::text(&datum, "proposal")?, law.proposal, "proposal")?;
    require(&parse::number(&datum, "reads")?, law.reads, "reads")?;
    require(
        &parse::number(&datum, "attempts")?,
        law.attempts,
        "attempts",
    )?;
    require(&parse::number(&datum, "bytes")?, law.bytes, "bytes")?;
    require(&parse::number(&datum, "files")?, law.files, "files")?;
    require(&parse::number(&datum, "tokens")?, law.tokens, "tokens")?;
    require(
        &parse::number(&datum, "worker-calls")?,
        law.workers,
        "workers",
    )?;
    require(
        &parse::number(&datum, "proposal-calls")?,
        law.proposals,
        "proposals",
    )?;
    require(&parse::number(&datum, "effects")?, "0", "effects")?;
    require(&parse::number(&datum, "subprocesses")?, "0", "subprocesses")?;
    if parse::text_bool(&datum, "resident")? != "true" {
        return Err(parse::fault("resident"));
    }
    let report = format!(
        "work {case} {} packet {} partial {} proposal {} reads {} attempts {} bytes {} files {} tokens {}",
        law.class,
        law.packet,
        law.partial,
        law.proposal,
        law.reads,
        law.attempts,
        law.bytes,
        law.files,
        law.tokens
    );
    Ok((law.class.to_owned(), seal_of(&report)))
}

fn sealed_gap(
    command: &crate::command::WorldCommandLib,
    case: &str,
    law: &GapLaw,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&argv(case))?;
    parse::refuse_copied(&datum)?;
    if parse::kind_of(&datum) != "world/work-gap-v1" {
        return Err(parse::fault("work-gap"));
    }
    require(&parse::text(&datum, "case")?, law.case, "case")?;
    require(&parse::text(&datum, "reason")?, law.reason, "reason")?;
    require(&parse::number(&datum, "reads")?, law.reads, "reads")?;
    require(
        &parse::number(&datum, "attempts")?,
        law.attempts,
        "attempts",
    )?;
    require(&parse::text(&datum, "evidence")?, law.evidence, "evidence")?;
    require(&parse::text(&datum, "mirrors")?, law.mirrors, "mirrors")?;
    require(
        &parse::number(&datum, "worker-calls")?,
        law.workers,
        "workers",
    )?;
    require(
        &parse::number(&datum, "proposal-calls")?,
        law.proposals,
        "proposals",
    )?;
    require(&parse::number(&datum, "effects")?, "0", "effects")?;
    require(&parse::number(&datum, "subprocesses")?, "0", "subprocesses")?;
    let report = format!(
        "gap {} {} reads {} attempts {} evidence {} mirrors {}",
        law.case, law.reason, law.reads, law.attempts, law.evidence, law.mirrors
    );
    Ok((law.reason.to_owned(), seal_of(&report)))
}

fn argv(case: &str) -> Vec<String> {
    vec!["sim".to_owned(), "work".to_owned(), case.to_owned()]
}

fn seal_of(report: &str) -> String {
    parse::hex(Sha256::digest(report.as_bytes()).as_slice())
}

fn hex_seal(value: &str) -> bool {
    value.len() == 64
        && value != ZERO_SEAL
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn require(found: &str, intended: &str, label: &str) -> Result<(), WorldError> {
    if found == intended {
        Ok(())
    } else {
        Err(parse::fault(label))
    }
}

#[cfg(test)]
#[path = "work_execute_tests.rs"]
mod tests;
