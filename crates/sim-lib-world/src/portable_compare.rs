// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cold comparison for the portable phase union.
//!
//! The leaf calls the loaded prove command. That cold run skips this gate
//! and keeps reuse disabled. It passes when false reuse is zero and the
//! frozen D1, D2, and D4 bounds hold.

// conformance: cold comparison checks the loaded prove command against the frozen bounds.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Mutex;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::{
    ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, execute_proof_leaf,
};

use crate::WorldError;
use crate::prove_command::ProveCommandLib;

pub(super) const ENTRY: &str = "cli/main/prove";
pub(super) const SKIP: &str = "phase/14";

#[path = "portable_compare_expect.rs"]
mod expect;
#[path = "portable_compare_parse.rs"]
mod parse;

pub(super) struct OwnedCompare {
    pub(super) leaves: String,
    pub(super) reuse: String,
    pub(super) false_reuse: String,
    pub(super) d1: String,
    pub(super) d2: String,
    pub(super) d4: String,
    pub(super) assay: String,
}

pub(super) struct ColdOthers {
    pub(super) denominator: u64,
    pub(super) executed: u64,
    pub(super) requests: u64,
    pub(super) gaps: u64,
    pub(super) failures: u64,
    pub(super) owner_commands: u64,
    pub(super) reuse_disabled: bool,
    pub(super) skipped: String,
    pub(super) resident: bool,
}

pub(super) struct AssayBounds {
    pub(super) denominator: u64,
    pub(super) d1: u64,
    pub(super) d2: u64,
    pub(super) d4: u64,
    pub(super) false_reuse: u64,
}

#[derive(Clone)]
pub(super) struct Capture {
    pub(super) cold: Datum,
    pub(super) assay: Datum,
    exports: bool,
}

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static STASH: RefCell<Option<super::UnionProof>> = const { RefCell::new(None) };
}

static CACHE: Mutex<Option<Capture>> = Mutex::new(None);

struct SurveyGuard;

impl SurveyGuard {
    fn enter() -> Self {
        INSIDE.with(|inside| inside.set(true));
        Self
    }
}

impl Drop for SurveyGuard {
    fn drop(&mut self) {
        INSIDE.with(|inside| inside.set(false));
        STASH.with(|stash| *stash.borrow_mut() = None);
    }
}

/// Builds the public union. The loaded cold run reads the stashed other leaves.
///
/// # Errors
/// Returns an error when a gate cannot be built or qualified.
pub(super) fn execute_union() -> Result<super::UnionProof, WorldError> {
    if inside_union() {
        return Err(super::qualify("prove cold comparison called itself"));
    }
    let _guard = SurveyGuard::enter();
    let leaves = super::phase_names()?;
    if cached().is_some() {
        return finish(&leaves, build_grounded(false)?);
    }
    let mut built = build_grounded(true)?;
    let others = other_leaves(&leaves)?;
    stash(finish(&others, built.clone())?);
    built.insert(SKIP.to_owned(), super::leaf_for(SKIP)?);
    finish(&leaves, built)
}

pub(super) fn compare_leaf() -> Result<ProofLeaf, WorldError> {
    Ok(leaf_from(survey()))
}

/// The stashed cold run over every gate except this one.
pub(super) fn cold_others_datum() -> Option<Datum> {
    STASH.with(|stash| stash.borrow().as_ref().map(others_datum))
}

pub(super) fn inside_union() -> bool {
    INSIDE.with(Cell::get)
}

fn build_grounded(skip_compare: bool) -> Result<BTreeMap<String, ProofLeaf>, WorldError> {
    let mut built = BTreeMap::new();
    for name in super::grounded_names()? {
        if skip_compare && name == SKIP {
            continue;
        }
        built.insert(name.to_owned(), super::leaf_for(name)?);
    }
    Ok(built)
}

fn other_leaves(all: &[String]) -> Result<Vec<String>, WorldError> {
    let leaves = all
        .iter()
        .filter(|name| name.as_str() != SKIP)
        .cloned()
        .collect::<Vec<_>>();
    if leaves.len().saturating_add(1) != super::PHASES {
        return Err(super::qualify(
            "prove cold comparison skipped the wrong gate",
        ));
    }
    Ok(leaves)
}

fn finish(
    leaves: &[String],
    built: BTreeMap<String, ProofLeaf>,
) -> Result<super::UnionProof, WorldError> {
    let order = ordered(&built);
    if order.len() != built.len() {
        return Err(super::qualify("unavailable-proof extra"));
    }
    for name in super::grounded_names()? {
        if leaves.iter().any(|leaf| leaf == name) && !built.contains_key(name) {
            return Err(super::qualify(format!("unavailable-proof {name}")));
        }
    }
    let catalog = ProofCatalog::new(built.into_values()).map_err(super::qualify)?;
    let launcher = super::RefuseLaunch;
    let cancellation = ProcessCancellation::default();
    let mut executed = Vec::with_capacity(order.len());
    let mut failures = Vec::new();
    let mut digests = BTreeMap::new();
    let mut retained = BTreeMap::new();
    let mut owner_commands = 0u64;
    let mut effects = 0u64;
    let mut subprocesses = 0u64;
    for name in &order {
        let leaf = catalog.leaf(name).map_err(super::qualify)?;
        if matches!(leaf, ProofLeaf::Command(_)) {
            owner_commands = owner_commands.saturating_add(1);
        }
        let receipt =
            execute_proof_leaf(&catalog, name, &launcher, &cancellation, "portable-union")
                .map_err(super::qualify)?;
        let work = crate::query::HostWork::observe(
            receipt.effect_id.as_deref(),
            receipt.launcher_identity.as_deref(),
        );
        effects = effects.saturating_add(work.effects());
        subprocesses = subprocesses.saturating_add(work.subprocesses());
        if receipt.disposition != ProofDisposition::Passed {
            failures.push((*name).to_owned());
        }
        let passed = receipt.disposition == ProofDisposition::Passed;
        digests.insert(
            (*name).to_owned(),
            super::digest_of(leaf, passed, &receipt.semantic_detail)?,
        );
        retained.insert(
            (*name).to_owned(),
            super::retained_capture(leaf, &receipt.semantic_detail)?,
        );
        executed.push((*name).to_owned());
    }
    Ok(super::UnionProof {
        subject: super::subject()?,
        leaves: leaves.to_vec(),
        executed,
        gaps: gaps_for(leaves)?,
        failures,
        requests: u64::try_from(order.len()).unwrap_or(u64::MAX),
        owner_commands,
        effects,
        subprocesses,
        digests,
        retained,
    })
}

fn ordered(built: &BTreeMap<String, ProofLeaf>) -> Vec<&'static str> {
    super::GATES
        .iter()
        .filter(|gate| built.contains_key(gate.leaf))
        .map(|gate| gate.leaf)
        .collect()
}

fn gaps_for(leaves: &[String]) -> Result<Vec<String>, WorldError> {
    let mut rows = Vec::new();
    for row in super::gap_rows()? {
        let Some(name) = row.split_whitespace().nth(1) else {
            continue;
        };
        if leaves.iter().any(|leaf| leaf == name) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn stash(proof: super::UnionProof) {
    STASH.with(|slot| *slot.borrow_mut() = Some(proof));
}

fn others_datum(proof: &super::UnionProof) -> Datum {
    let skipped = if includes_self(proof) {
        "self-included"
    } else {
        SKIP
    };
    super::node(
        "cold-others-v1",
        vec![
            ("denominator", super::count(proof.leaves.len())),
            ("effects", super::num(proof.effects)),
            ("executed", super::count(proof.executed.len())),
            ("failures", super::count(proof.failures.len())),
            ("gaps", super::count(proof.gaps.len())),
            ("owner-commands", super::num(proof.owner_commands)),
            ("requests", super::num(proof.requests)),
            ("resident", super::flag(true)),
            ("reuse-disabled", super::flag(true)),
            ("skipped", super::text(skipped)),
            ("subprocesses", super::num(proof.subprocesses)),
        ],
    )
}

fn includes_self(proof: &super::UnionProof) -> bool {
    let named = |items: &[String]| items.iter().any(|item| item == SKIP);
    named(&proof.leaves)
        || named(&proof.executed)
        || proof
            .gaps
            .iter()
            .any(|gap| gap.split_whitespace().nth(1) == Some(SKIP))
}

fn survey() -> Capture {
    if let Some(found) = cached() {
        return found;
    }
    let fresh = load_capture();
    if parse::parsed_compare(&fresh).is_some() {
        store(fresh.clone());
    }
    fresh
}

fn cached() -> Option<Capture> {
    CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

fn store(capture: Capture) {
    let mut slot = CACHE.lock().unwrap_or_else(|poison| poison.into_inner());
    if slot.is_none() {
        *slot = Some(capture);
    }
}

fn load_capture() -> Capture {
    let prove = ProveCommandLib::new();
    let exports = prove.manifest().exports.iter().any(|export| {
        matches!(
            export,
            Export::Function { symbol, .. } if symbol.to_string() == ENTRY
        )
    });
    if inside_union() && cold_others_datum().is_none() {
        return Capture {
            cold: missing(),
            assay: missing(),
            exports,
        };
    }
    let cold = prove
        .run(&words(&["sim", "run", "cold"]))
        .unwrap_or_else(|_| missing());
    let assay = super::project::resident_command()
        .and_then(|command| command.run(&words(&["sim", "assay"])))
        .unwrap_or_else(|_| missing());
    Capture {
        cold,
        assay,
        exports,
    }
}

fn words(values: &[&str]) -> Vec<String> {
    values.iter().copied().map(str::to_owned).collect()
}

fn missing() -> Datum {
    super::gap("compare", "missing")
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    let actual = actual_deck(&capture);
    #[cfg(test)]
    REMEMBERED.with(|slot| *slot.borrow_mut() = Some(actual.clone()));
    ProofLeaf::SourceDeckPredicate {
        name: SKIP.to_owned(),
        actual_deck: actual,
        expected_deck: expect::expected_deck(),
        required_claims: expect::required_ids(),
        present_claims: present_claims(&capture),
    }
}

fn actual_deck(capture: &Capture) -> String {
    let entry = if capture.exports { ENTRY } else { "missing" };
    let mut lines = vec![
        format!("entry {entry}"),
        format!("effects {}", parse::effect_total(capture)),
        format!("subprocesses {}", parse::subprocess_total(capture)),
        format!("resident {}", resident(capture)),
    ];
    if let Some(parsed) = parse::parsed_compare(capture) {
        lines.push(expect::compare_line(&parsed));
    } else {
        lines.push("compare missing".to_owned());
    }
    lines.join("\n")
}

fn present_claims(capture: &Capture) -> std::collections::BTreeSet<String> {
    let mut claims = std::collections::BTreeSet::new();
    if let Some(parsed) = parse::parsed_compare(capture) {
        claims.insert(expect::compare_claim(&parsed));
    }
    claims
}

fn resident(capture: &Capture) -> &'static str {
    match (
        flag(&capture.cold, "resident"),
        flag(&capture.assay, "resident"),
    ) {
        (Some(true), Some(true)) => "true",
        (Some(false), _) | (_, Some(false)) => "false",
        _ => "missing",
    }
}

fn flag(value: &Datum, name: &str) -> Option<bool> {
    let Datum::Node { fields, .. } = value else {
        return None;
    };
    let field = fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, field)| field)?;
    match field {
        Datum::Bool(bit) => Some(*bit),
        _ => None,
    }
}

#[cfg(test)]
thread_local! {
    static REMEMBERED: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn last_deck() -> String {
    REMEMBERED.with(|slot| slot.borrow().clone().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_cold_comparison_matches_the_frozen_bounds() {
        let proof = super::super::execute().expect("union");
        let deck = super::last_deck();
        assert_eq!(deck, super::expect::expected_deck(), "{deck}");
        assert_eq!(proof.executed.len(), 28, "{deck}");
        assert!(
            !proof.gaps.iter().any(|gap| gap.contains(super::SKIP)),
            "{deck}"
        );
    }
}
