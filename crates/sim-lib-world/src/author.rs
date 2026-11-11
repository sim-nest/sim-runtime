// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded author command.
//!
//! Each case calls the authoring compiler once. A later or concurrent
//! identical case returns that record. A consulted or superseded sibling
//! does not satisfy a requirement the selected source omits. A missing
//! section refuses in the parser. A decision that names begin authority
//! refuses. A private value is refused before the reservation.

use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    AuthorError, StatementKind, compile_authoring, diff_members, lint_family, list_family,
    parse_member, select_member, show_member, source_round_trip, statement_shape,
};

use crate::WorldError;
use crate::query;

#[path = "author_ground.rs"]
mod ground;
#[path = "author_memory.rs"]
mod memory;

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

/// Runs one named authoring case through the authoring compiler.
///
/// # Errors
/// Returns an error when the compiler record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if leaked(case) {
        return Ok(quiet("", "private-leakage"));
    }
    if !known(case) {
        return Ok(quiet(case, "unknown-case"));
    }
    let claim = match memory::claim(case) {
        memory::Reservation::Ready(found) => return Ok(found),
        memory::Reservation::Mine(claim) => claim,
    };
    let rendered = receive(canonical_case(case));
    claim.commit(&rendered);
    Ok(rendered)
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "selected"
            | "consulted"
            | "superseded"
            | "argument"
            | "work"
            | "gate"
            | "ratchet"
            | "owner"
            | "checker"
            | "authority"
    ) || cfg!(test) && case == "author-memory-race"
}

/// Maps the reservation-race case onto the selected family.
///
/// The race key is not a second authoring law. Tests use it so two calls of
/// the loaded command share one compiler record.
fn canonical_case(case: &str) -> &str {
    #[cfg(test)]
    if case == "author-memory-race" {
        return "selected";
    }
    case
}

fn receive(case: &str) -> Datum {
    match case {
        "selected" => selected(),
        "consulted" | "superseded" => sibling(case),
        "argument" | "work" | "gate" | "ratchet" | "owner" | "checker" => missing(case),
        "authority" => authority(),
        _ => quiet(case, "unknown-case"),
    }
}

fn selected() -> Datum {
    let notes = ground::notes();
    let chosen = ground::chosen();
    let family = match ground::family(&[notes.clone(), chosen.clone()]) {
        Ok(family) => family,
        Err(error) => return refused("selected", &error),
    };
    let deck = ground::deck(&[
        ("family/notes.txt", notes.as_slice()),
        ("family/chosen.txt", chosen.as_slice()),
    ]);
    let spec = ground::spec();
    let listed = match list_family(&family, &deck, &spec) {
        Ok(listed) => listed,
        Err(error) => return refused("selected", &error),
    };
    let view = match show_member(&family, "chosen", &deck, &spec) {
        Ok(view) => view,
        Err(error) => return refused("selected", &error),
    };
    if let Err(error) = lint_family(&family, &deck, &spec) {
        return refused("selected", &error);
    }
    let selection = match select_member(&family, &deck, &spec) {
        Ok(selection) => selection,
        Err(error) => return refused("selected", &error),
    };
    let diff = match diff_members(&family, "chosen", "notes", &deck, &spec) {
        Ok(diff) => diff,
        Err(error) => return refused("selected", &error),
    };
    let Some(member) = family.members().iter().find(|item| item.name == "chosen") else {
        return quiet("selected", "member-missing");
    };
    let round_trip =
        source_round_trip(member) == member.source.as_slice() && selection.source == member.source;
    let advice = statement_shape(StatementKind::Advice);
    let fact = statement_shape(StatementKind::Fact);
    let decision = statement_shape(StatementKind::Decision);
    let distinct = advice.expected() != fact.expected()
        && fact.expected() != decision.expected()
        && advice.expected() != decision.expected();
    let compiled = match compile_authoring(&family, &deck, &spec) {
        Ok(compiled) => compiled,
        Err(error) => return refused("selected", &error),
    };
    let changed = diff.changed.join(",");
    let grounded = view.in_deck
        && view.roadmap_matches
        && diff.left_in_deck
        && diff.right_in_deck
        && diff.left_roadmap_matches
        && diff.right_roadmap_matches;
    let closed = compiled
        .revision
        .claims()
        .values()
        .all(|claim| claim.closed);
    success(Record {
        case: "selected",
        members: listed.len(),
        shown: &view.member.name,
        selection: &selection.name,
        changed: &changed,
        round_trip,
        distinct,
        consultation: compiled.consultation.len(),
        bytes: member.source.len(),
        proof: compiled.proof.name(),
        phase: &compiled.work.phase,
        uncertain: compiled.revision.is_uncertain(),
        closed,
        grounded,
        claims: compiled.revision.claims().len(),
        entries: compiled.mutation.entries.len(),
        postimage: compiled.mutation.total_postimage_bytes,
    })
}

fn sibling(case: &str) -> Datum {
    let chosen = ground::member(
        "chosen",
        "2",
        "selected",
        "family/chosen.txt",
        false,
        "select this source",
        None,
    );
    let notes = ground::member(
        "notes",
        "1",
        case,
        "family/notes.txt",
        true,
        "select this source",
        None,
    );
    compile_refusal(
        case,
        &[notes.clone(), chosen.clone()],
        &[
            ("family/notes.txt", notes.as_slice()),
            ("family/chosen.txt", chosen.as_slice()),
        ],
    )
}

fn authority() -> Datum {
    let chosen = ground::bytes(ground::Draft {
        name: "chosen",
        round: "2",
        stance: "selected",
        path: "family/authority.txt",
        fact: true,
        decision: "begin authority",
        consult: None,
    });
    compile_refusal(
        "authority",
        std::slice::from_ref(&chosen),
        &[("family/authority.txt", chosen.as_slice())],
    )
}

fn compile_refusal(case: &str, parts: &[Vec<u8>], files: &[(&str, &[u8])]) -> Datum {
    let family = match ground::family(parts) {
        Ok(family) => family,
        Err(error) => return refused(case, &error),
    };
    let deck = ground::deck(files);
    let spec = ground::spec();
    match compile_authoring(&family, &deck, &spec) {
        Ok(_) => quiet(case, "compiled"),
        Err(error) => refused(case, &error),
    }
}

fn missing(section: &str) -> Datum {
    let bytes = ground::without(&ground::chosen(), section);
    match parse_member(&bytes) {
        Ok(_) => quiet(section, "parsed"),
        Err(error) => refused(section, &error),
    }
}

struct Record<'a> {
    case: &'a str,
    members: usize,
    shown: &'a str,
    selection: &'a str,
    changed: &'a str,
    round_trip: bool,
    distinct: bool,
    consultation: usize,
    bytes: usize,
    proof: &'a str,
    phase: &'a str,
    uncertain: bool,
    closed: bool,
    grounded: bool,
    claims: usize,
    entries: usize,
    postimage: usize,
}

fn success(record: Record<'_>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "author-v1"),
        fields: vec![
            (Symbol::new("bytes"), width(record.bytes)),
            (Symbol::new("case"), string(shown(record.case))),
            (Symbol::new("changed"), string(shown(record.changed))),
            (Symbol::new("claims"), width(record.claims)),
            (Symbol::new("class"), string("selected")),
            (Symbol::new("closed"), string(flag(record.closed))),
            (Symbol::new("compiled"), string("true")),
            (Symbol::new("consultation"), width(record.consultation)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("entries"), width(record.entries)),
            (Symbol::new("grounded"), string(flag(record.grounded))),
            (Symbol::new("lint"), string("ok")),
            (Symbol::new("members"), width(record.members)),
            (Symbol::new("phase"), string(shown(record.phase))),
            (Symbol::new("postimage"), width(record.postimage)),
            (Symbol::new("proof"), string(shown(record.proof))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("round-trip"), string(flag(record.round_trip))),
            (Symbol::new("selection"), string(shown(record.selection))),
            (
                Symbol::new("shapes"),
                string(if record.distinct { "distinct" } else { "same" }),
            ),
            (Symbol::new("shown"), string(shown(record.shown))),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("uncertain"), string(flag(record.uncertain))),
        ],
    }
}

fn quiet(case: &str, reason: &str) -> Datum {
    gap(case, reason, "")
}

fn refused(case: &str, error: &AuthorError) -> Datum {
    let (reason, evidence) = explain(error);
    gap(case, &reason, &evidence)
}

fn gap(case: &str, reason: &str, evidence: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "author-gap-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("evidence"), string(shown(evidence))),
            (Symbol::new("reason"), string(shown(reason))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn explain(error: &AuthorError) -> (String, String) {
    match error {
        AuthorError::Utf8 => ("utf8".to_owned(), String::new()),
        AuthorError::Header => ("header".to_owned(), String::new()),
        AuthorError::Missing { section } => ("missing".to_owned(), (*section).to_owned()),
        AuthorError::Duplicate { section } => ("duplicate".to_owned(), (*section).to_owned()),
        AuthorError::UnknownField { name } => ("unknown-field".to_owned(), name.clone()),
        AuthorError::Stance => ("stance".to_owned(), String::new()),
        AuthorError::Path => ("path".to_owned(), String::new()),
        AuthorError::Identity { kind } => ("identity".to_owned(), (*kind).to_owned()),
        AuthorError::EmptyFamily => ("empty-family".to_owned(), String::new()),
        AuthorError::DuplicateMember { kind } => {
            ("duplicate-member".to_owned(), (*kind).to_owned())
        }
        AuthorError::SelectionCount { count } => ("selection-count".to_owned(), count.to_string()),
        AuthorError::DeckMismatch { path } => ("deck-mismatch".to_owned(), path.clone()),
        AuthorError::RoadmapMismatch { found, spec } => {
            ("roadmap-mismatch".to_owned(), format!("{found} {spec}"))
        }
        AuthorError::Ungrounded => ("ungrounded".to_owned(), String::new()),
        AuthorError::Unsatisfied { requirement } => ("unsatisfied".to_owned(), requirement.clone()),
        AuthorError::Authority => ("authority".to_owned(), String::new()),
        AuthorError::Consult { round } => ("consult".to_owned(), round.clone()),
        AuthorError::MemberMissing { name } => ("member-missing".to_owned(), name.clone()),
        AuthorError::Roadmap(text) => ("roadmap".to_owned(), text.clone()),
        AuthorError::Revision(text) => ("revision".to_owned(), text.clone()),
        AuthorError::Proof(text) => ("proof".to_owned(), text.clone()),
        AuthorError::Mutation(text) => ("mutation".to_owned(), text.clone()),
    }
}

fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn width(value: usize) -> Datum {
    query::count_datum(u64::try_from(value).unwrap_or(u64::MAX))
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}
