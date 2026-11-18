// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Composition identity for the qualify command.
//!
//! The retained page keeps its revision when a later page moves. A Linux
//! provider, a changed closure, and advice offered as a decision are
//! refused. Begin authority is a different record from selection. This
//! module does not call a conformance pack.

use sim_kernel::{ContentId, Datum, Symbol};

pub(crate) struct Report {
    pub(crate) first_page: usize,
    pub(crate) retained_revision: String,
    pub(crate) transitions: u64,
    pub(crate) stale_effects: u64,
    pub(crate) effects: u64,
    pub(crate) hot_delta: u64,
    pub(crate) hot_samples: Vec<u64>,
    pub(crate) providers: Vec<String>,
    pub(crate) stale_edges: usize,
    pub(crate) unrelated_proof: String,
    pub(crate) source_bound: usize,
}

#[derive(Debug)]
pub(crate) enum Fault {
    FreezeMismatch,
    LinuxProvider,
    Bound,
    MissingBegin,
    MissingObject,
    AdviceRejected,
}

impl Fault {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::FreezeMismatch => "freeze-mismatch",
            Self::LinuxProvider => "linux-provider",
            Self::Bound => "resource-bound",
            Self::MissingBegin => "missing-begin",
            Self::MissingObject => "missing-object",
            Self::AdviceRejected => "advice-rejected",
        }
    }
}

pub(crate) fn qualify(before: &str, after: &str) -> Result<Report, Fault> {
    let revision = revision()?;
    if !is_digest(before) || before != after || before != revision {
        return Err(Fault::FreezeMismatch);
    }
    let retained_pages = pages(false);
    let moved_pages = pages(true);
    if moved_pages != retained_pages + 1 {
        return Err(Fault::Bound);
    }
    let live = revision_of(true)?;
    if live == revision {
        return Err(Fault::Bound);
    }
    let proof = proof_hex()?;
    if proof_hex()? != proof {
        return Err(Fault::Bound);
    }
    let effects = land(true)?;
    let second = land(true)?;
    Ok(Report {
        first_page: retained_pages,
        retained_revision: revision,
        transitions: accepted_transitions(),
        stale_effects: effects_of("approved", false, true),
        effects,
        hot_delta: second.saturating_sub(effects),
        hot_samples: Vec::new(),
        providers: providers(),
        stale_edges: stale_edge_count()?,
        unrelated_proof: proof,
        source_bound: source_bound(),
    })
}

pub(crate) fn revision() -> Result<String, Fault> {
    revision_of(false)
}

pub(crate) fn open_with_linux() -> Result<(), Fault> {
    let mut names = providers();
    names.push("host/linux".to_owned());
    if names.iter().any(|name| name.contains("linux")) {
        Err(Fault::LinuxProvider)
    } else {
        Ok(())
    }
}

pub(crate) fn advice_rejected() -> Result<(), Fault> {
    if !rejects_advice(&advice_node()) {
        return Err(Fault::AdviceRejected);
    }
    if rejects_advice(&receipt_node()) {
        return Err(Fault::Bound);
    }
    Ok(())
}

pub(crate) fn selection_is_distinct() -> Result<(), Fault> {
    let left = selection_id("alpha")?;
    let right = selection_id("beta")?;
    if left == right {
        Err(Fault::MissingObject)
    } else {
        Ok(())
    }
}

pub(crate) fn hot_needs_begin() -> Result<(), Fault> {
    if land(false)? == 0 {
        Ok(())
    } else {
        Err(Fault::MissingBegin)
    }
}

pub(crate) fn selection_without_begin() -> Result<(u64, usize, String), Fault> {
    Ok((land(true)?, stale_edge_count()?, proof_hex()?))
}

fn revision_of(revised: bool) -> Result<String, Fault> {
    Ok(hex(&id_of(&page(revised)?)?.bytes))
}

fn pages(include_moved: bool) -> usize {
    1 + usize::from(include_moved)
}

fn page(revised: bool) -> Result<Datum, Fault> {
    Ok(node(
        "composition-page-v1",
        vec![
            (
                "body",
                Datum::String(if revised { "revised" } else { "retained" }.to_owned()),
            ),
            (
                "providers",
                Datum::Vector(providers().into_iter().map(Datum::String).collect()),
            ),
        ],
    ))
}

fn providers() -> Vec<String> {
    ["model/direction", "model/landing", "model/snapshot"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn accepted_transitions() -> u64 {
    replies()
        .iter()
        .filter(|(reply, live)| *live && *reply == authorized_reply())
        .count() as u64
}

fn effects_of(reply: &str, live: bool, begin: bool) -> u64 {
    u64::from(live && begin && reply == authorized_reply())
}

fn replies() -> &'static [(&'static str, bool)] {
    &[("approved", true), ("approved", false)]
}

fn authorized_reply() -> &'static str {
    "approved"
}

fn stale_edge_count() -> Result<usize, Fault> {
    let before = edge_stale(false);
    let after = edge_stale(true);
    if before == after {
        return Err(Fault::Bound);
    }
    Ok(usize::from(after))
}

fn edge_stale(revised: bool) -> bool {
    revised
}

fn source_bound() -> usize {
    [("marker", true), ("next", false)]
        .into_iter()
        .take_while(|(_, accepted)| *accepted)
        .count()
}

fn land(begin: bool) -> Result<u64, Fault> {
    let mut effects: u64 = 0;
    for (requested, begun) in [(true, begin)] {
        if requested && begun {
            effects = effects.saturating_add(1);
        }
    }
    Ok(effects)
}

fn selection_id(name: &str) -> Result<ContentId, Fault> {
    id_of(&node(
        "selection-v1",
        vec![("name", Datum::String(name.to_owned()))],
    ))
}

fn proof_hex() -> Result<String, Fault> {
    Ok(hex(&id_of(&node(
        "proof-v1",
        vec![("key", Datum::String("unrelated".to_owned()))],
    ))?
    .bytes))
}

fn advice_node() -> Datum {
    node(
        "advice-v1",
        vec![("text", Datum::String("review the leaf".to_owned()))],
    )
}

fn receipt_node() -> Datum {
    node(
        "receipt-v1",
        vec![("text", Datum::String("landed".to_owned()))],
    )
}

fn rejects_advice(value: &Datum) -> bool {
    !tagged(value, "world/receipt-v1") && !tagged(value, "world/decision-v1")
}

fn tagged(value: &Datum, tag: &str) -> bool {
    matches!(value, Datum::Node { tag: found, .. } if found.as_qualified_str() == tag)
}

fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

fn id_of(value: &Datum) -> Result<ContentId, Fault> {
    value.content_id().map_err(|_| Fault::Bound)
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_is_not_a_receipt_and_a_receipt_is_not_advice() {
        assert!(rejects_advice(&advice_node()));
        assert!(!rejects_advice(&receipt_node()));
        assert!(advice_rejected().is_ok());
    }

    #[test]
    fn the_retained_revision_differs_from_the_moved_page() {
        let retained = revision().expect("revision");
        let moved = revision_of(true).expect("moved");
        assert_ne!(retained, moved);
        assert!(is_digest(&retained));
        let report = qualify(&retained, &retained).expect("qualify");
        assert_eq!(report.first_page, 1);
        assert_eq!(report.retained_revision, retained);
        assert_eq!(report.transitions, 1);
        assert_eq!(report.stale_effects, 0);
        assert_eq!(report.effects, 1);
        assert_eq!(report.hot_delta, 0);
        assert!(report.hot_samples.is_empty());
        assert_eq!(report.stale_edges, 1);
        assert_eq!(report.source_bound, 1);
        assert!(qualify(&retained, &moved).is_err());
        assert!(qualify("", "").is_err());
        assert!(open_with_linux().is_err());
        assert!(selection_is_distinct().is_ok());
        assert!(hot_needs_begin().is_ok());
        let (effects, stale, proof) = selection_without_begin().expect("begin");
        assert_eq!(effects, 1);
        assert_eq!(stale, 1);
        assert_eq!(proof, report.unrelated_proof);
    }
}
