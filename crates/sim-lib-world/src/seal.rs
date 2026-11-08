// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Offline seal of one source universe.
//!
//! The owning batch is the resolver input captured while this crate compiled:
//! workspace lock, member features, nested Cargo locks, generated sources, and
//! toolchain identity. A loaded command reads that sealed text and does not
//! open a compiler, a host process, or a network connection. Only a verified
//! input admits compilation. Present-unverified, stub-only, and absent stay
//! distinct, and a target-specific status is never copied onto another target.

use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::Datum;

use crate::WorldError;
use crate::provider::content_id;
use crate::query::{self, CEILING_MS, SAMPLES, WARMUPS};

mod batch;
mod emit;
mod marks;
mod parse;
/// Admission record for one batch and requested target.
///
/// # Errors
/// Returns an error when the sealed input text is internally inconsistent.
#[cfg(test)]
pub(crate) fn open(batch: &str, target: Option<&str>) -> Result<Datum, WorldError> {
    Ok(assess(batch, target)?.seal)
}

/// Assess caller-supplied sealed input text.
///
/// # Errors
/// Returns an error when the text is internally inconsistent.
#[cfg(test)]
pub(crate) fn open_inputs(text: &str) -> Result<Datum, WorldError> {
    Ok(assess_text("inputs", text, None)?.seal)
}

/// Source-universe record for caller-supplied sealed input text.
///
/// # Errors
/// Returns an error when the text is internally inconsistent.
#[cfg(test)]
pub(crate) fn universe_inputs(text: &str) -> Result<Datum, WorldError> {
    Ok(assess_text("inputs", text, None)?.universe)
}

/// Source-universe record for one batch and requested target.
///
/// # Errors
/// Returns an error when the sealed input text is internally inconsistent.
#[cfg(test)]
pub(crate) fn universe(batch: &str, target: Option<&str>) -> Result<Datum, WorldError> {
    Ok(assess(batch, target)?.universe)
}

/// Names the resolver inputs that differ between two batches.
///
/// # Errors
/// Returns an error when either sealed input text is internally inconsistent.
#[cfg(test)]
pub(crate) fn diff(before: &str, after: &str, target: Option<&str>) -> Result<Datum, WorldError> {
    let left = assess(before, target)?;
    let right = assess(after, target)?;
    if left.gap || right.gap {
        return Ok(emit::gap("unknown-batch", before));
    }
    let inputs = marks::changed(&left.marks, &right.marks);
    Ok(emit::diff_datum(&left.digest, &right.digest, &inputs))
}

/// Resolver text captured for this crate. This is the admitted input object,
/// not a batch name.
#[must_use]
pub fn captured_inputs() -> &'static str {
    batch::captured()
}

/// Workspace whose lock and members produced [`captured_inputs`].
#[must_use]
pub fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Seals one admitted input document. `None` selects the captured workspace.
///
/// A value that is not `sim-world-inputs-v1` text is an unknown batch. Named
/// examples are not consulted.
///
/// # Errors
/// Returns an error when the admitted text is internally inconsistent.
pub(crate) fn open_admitted(
    input: Option<&str>,
    target: Option<&str>,
) -> Result<Datum, WorldError> {
    Ok(assess_admitted(input, target)?.seal)
}

/// Source-universe record for one admitted input document.
///
/// # Errors
/// Returns an error when the admitted text is internally inconsistent.
pub(crate) fn universe_admitted(
    input: Option<&str>,
    target: Option<&str>,
) -> Result<Datum, WorldError> {
    Ok(assess_admitted(input, target)?.universe)
}

/// Names the resolver inputs that differ between two admitted documents.
///
/// # Errors
/// Returns an error when either admitted text is internally inconsistent.
pub(crate) fn diff_admitted(
    before: &str,
    after: &str,
    target: Option<&str>,
) -> Result<Datum, WorldError> {
    let left = assess_admitted(Some(before), target)?;
    let right = assess_admitted(Some(after), target)?;
    if left.gap || right.gap {
        return Ok(emit::gap("unknown-batch", before));
    }
    let inputs = marks::changed(&left.marks, &right.marks);
    Ok(emit::diff_datum(&left.digest, &right.digest, &inputs))
}

/// Times the owning seal. The command still opens no compiler.
///
/// # Errors
/// Returns an error when the owning input text is internally inconsistent.
pub(crate) fn measure(effects: usize) -> Result<Datum, WorldError> {
    let expected = open_admitted(None, None)?;
    let rendered = crate::command::render(&expected);
    for _ in 1..WARMUPS {
        if crate::command::render(&open_admitted(None, None)?) != rendered {
            return Err(WorldError::Qualification(
                "source seal changed during warmup".to_owned(),
            ));
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = std::time::Instant::now();
        let value = open_admitted(None, None)?;
        let elapsed = started.elapsed();
        if crate::command::render(&value) != rendered {
            return Err(WorldError::Qualification(
                "source seal changed during sampling".to_owned(),
            ));
        }
        samples.push(elapsed);
    }
    samples.sort_unstable();
    let p95_ms =
        u64::try_from(samples[query::nearest_rank_p95(SAMPLES)].as_millis()).unwrap_or(u64::MAX);
    let members = count_field(&expected, "members")?;
    let domains = count_field(&expected, "domains")?;
    let record = query::latency_record("seal", members, domains, p95_ms, effects);
    if p95_ms > CEILING_MS {
        return Err(WorldError::Qualification(format!(
            "source seal p95 {p95_ms} ms exceeded {CEILING_MS}"
        )));
    }
    Ok(record)
}

struct Assessed {
    seal: Datum,
    universe: Datum,
    digest: String,
    gap: bool,
    marks: BTreeMap<String, String>,
}

struct Inputs {
    config: BTreeMap<String, String>,
    toolchain: BTreeMap<String, String>,
    features: BTreeSet<String>,
    activates: BTreeSet<String>,
    patches: BTreeSet<String>,
    sealed_target: String,
    domains: BTreeMap<String, Domain>,
    members: Vec<Member>,
    generated: Vec<Generated>,
}

struct Domain {
    canonical: String,
    role: String,
    lock: String,
    aliases: BTreeSet<String>,
}

struct Member {
    domain: String,
    origin: String,
    name: String,
    version: String,
    source: String,
    checksum: String,
    kind: String,
    closure: bool,
    observed: String,
    compiling: bool,
    deps: Vec<String>,
    files: Option<u64>,
}

struct Generated {
    domain: String,
    path: String,
    bytes: u64,
    checksum: String,
}

struct Shown {
    status: &'static str,
    reason: Option<String>,
    content: Option<String>,
    checksum: Option<String>,
    checksum_kind: Option<String>,
    files: Option<u64>,
}

fn assess_admitted(input: Option<&str>, target: Option<&str>) -> Result<Assessed, WorldError> {
    let Some(text) = input else {
        return assess_text("admitted", batch::captured(), target);
    };
    if let Some(text) = batch::document_text(text) {
        return assess_text("admitted", &text, target);
    }
    let gap_value = emit::gap("unknown-batch", text);
    Ok(Assessed {
        seal: gap_value.clone(),
        universe: gap_value,
        digest: String::new(),
        gap: true,
        marks: BTreeMap::new(),
    })
}

#[cfg(test)]
fn assess(batch: &str, target: Option<&str>) -> Result<Assessed, WorldError> {
    let Some(text) = batch::batch_text(batch)? else {
        let gap_value = emit::gap("unknown-batch", batch);
        return Ok(Assessed {
            seal: gap_value.clone(),
            universe: gap_value,
            digest: String::new(),
            gap: true,
            marks: BTreeMap::new(),
        });
    };
    assess_text(batch, &text, target)
}

fn assess_text(batch: &str, text: &str, target: Option<&str>) -> Result<Assessed, WorldError> {
    let inputs = parse::parse(text)?;
    let requested = target.unwrap_or(inputs.sealed_target.as_str()).to_owned();
    let wrong_target = requested != inputs.sealed_target;
    let mut shown = Vec::new();
    let marks = marks::field_marks(&inputs);
    let mut refusal: Option<(&'static str, String)> = None;
    if wrong_target && let Some(member) = inputs.members.iter().find(|member| member.compiling) {
        refusal = Some((
            "wrong-target",
            format!("{} {} {}", member.domain, member.name, member.version),
        ));
    }
    for member in &inputs.members {
        let sealed = sealed_status(member);
        if !wrong_target
            && refusal.is_none()
            && let Some(reason) = sealed.refusal
        {
            refusal = Some((
                reason,
                format!("{} {} {}", member.domain, member.name, member.version),
            ));
        }
        let mut display = if wrong_target {
            Shown {
                status: "absent",
                reason: None,
                content: None,
                checksum: None,
                checksum_kind: None,
                files: None,
            }
        } else {
            sealed.shown
        };
        if !wrong_target {
            display.files = member.files;
        }
        shown.push(display);
    }
    if !wrong_target && refusal.is_none() && !inputs.members.iter().any(|member| member.compiling) {
        refusal = Some(("absent-input", "compiling".to_owned()));
    }
    let digest = hex_id(Datum::String(marks::digest_body(&marks)))?;
    let admission = if refusal.is_some() { "refuse" } else { "admit" };
    let counts = counts_of(&shown);
    let seal = emit::seal_datum(
        batch,
        admission,
        refusal.as_ref(),
        &digest,
        &inputs,
        &requested,
        counts,
    );
    let universe = emit::universe_datum(seal.clone(), &inputs, &requested, &shown);
    Ok(Assessed {
        seal,
        universe,
        digest,
        gap: false,
        marks,
    })
}

struct Sealed {
    shown: Shown,
    refusal: Option<&'static str>,
}

fn sealed_status(member: &Member) -> Sealed {
    if member.origin == "stub" {
        let reason = if member.checksum == "-" {
            "metadata-only"
        } else {
            member.checksum.as_str()
        };
        return Sealed {
            shown: Shown {
                status: "stub-only",
                reason: Some(reason.to_owned()),
                content: None,
                checksum: None,
                checksum_kind: None,
                files: None,
            },
            refusal: Some("stub-only"),
        };
    }
    if member.origin == "absent" {
        return absent_status("absent-input");
    }
    if member.kind == "version-mismatch" {
        return Sealed {
            shown: Shown {
                status: "present-unverified",
                reason: Some("version-mismatch".to_owned()),
                content: Some(format!(
                    "{} {} {} {} {}",
                    member.domain, member.origin, member.name, member.observed, member.source
                )),
                checksum: Some(member.checksum.clone()),
                checksum_kind: Some("path-content".to_owned()),
                files: None,
            },
            refusal: Some("version-mismatch"),
        };
    }
    if member.kind == "symlink-escape" {
        return Sealed {
            shown: Shown {
                status: "present-unverified",
                reason: Some("symlink-escape".to_owned()),
                content: Some(member.observed.clone()),
                checksum: None,
                checksum_kind: None,
                files: None,
            },
            refusal: Some("symlink-escape"),
        };
    }
    if member.checksum == "-" || member.kind == "none" {
        return Sealed {
            shown: Shown {
                status: "present-unverified",
                reason: Some("missing-checksum".to_owned()),
                content: None,
                checksum: None,
                checksum_kind: None,
                files: None,
            },
            refusal: Some("missing-input"),
        };
    }
    if member.origin == "registry"
        || member.origin == "git"
        || member.kind == "registry"
        || member.kind == "git"
    {
        return lock_bytes(member);
    }
    if !member.observed.is_empty() && member.observed != member.checksum {
        return corrupt_member(member);
    }
    verified_member(member)
}

fn lock_bytes(member: &Member) -> Sealed {
    if member.observed.is_empty() {
        return Sealed {
            shown: Shown {
                status: "present-unverified",
                reason: Some("source-not-reached".to_owned()),
                content: Some(member_key(member)),
                checksum: Some(member.checksum.clone()),
                checksum_kind: Some(member.kind.clone()),
                files: None,
            },
            refusal: Some("source-not-reached"),
        };
    }
    if member.observed != member.checksum {
        return corrupt_member(member);
    }
    verified_member(member)
}

fn corrupt_member(member: &Member) -> Sealed {
    Sealed {
        shown: Shown {
            status: "present-unverified",
            reason: Some("corrupt-checksum".to_owned()),
            content: Some(member.observed.clone()),
            checksum: Some(member.checksum.clone()),
            checksum_kind: Some(member.kind.clone()),
            files: None,
        },
        refusal: Some("corrupt-input"),
    }
}

fn verified_member(member: &Member) -> Sealed {
    Sealed {
        shown: Shown {
            status: "verified",
            reason: None,
            content: Some(member_key(member)),
            checksum: Some(member.checksum.clone()),
            checksum_kind: Some(member.kind.clone()),
            files: None,
        },
        refusal: None,
    }
}

fn absent_status(reason: &'static str) -> Sealed {
    Sealed {
        shown: Shown {
            status: "absent",
            reason: None,
            content: None,
            checksum: None,
            checksum_kind: None,
            files: None,
        },
        refusal: Some(reason),
    }
}

fn counts_of(shown: &[Shown]) -> [usize; 4] {
    let mut counts = [0, 0, 0, 0];
    for row in shown {
        let slot = match row.status {
            "verified" => 0,
            "present-unverified" => 1,
            "stub-only" => 2,
            _ => 3,
        };
        counts[slot] += 1;
    }
    counts
}

fn member_key(member: &Member) -> String {
    format!(
        "{} {} {} {} {}",
        member.domain, member.origin, member.name, member.version, member.source
    )
}

fn count_field(value: &Datum, name: &str) -> Result<usize, WorldError> {
    let Datum::Node { fields, .. } = value else {
        return Err(WorldError::Qualification("seal is not a node".to_owned()));
    };
    let Some((_, field)) = fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
    else {
        return Err(WorldError::Qualification(format!("seal is missing {name}")));
    };
    let Datum::Number(number) = field else {
        return Err(WorldError::Qualification(format!("{name} is not a count")));
    };
    number
        .canonical
        .parse()
        .map_err(|_| WorldError::Qualification(format!("{name} is not a count")))
}

fn hex_id(value: Datum) -> Result<String, WorldError> {
    let id = content_id(value)?;
    Ok(hex(&id.bytes))
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
