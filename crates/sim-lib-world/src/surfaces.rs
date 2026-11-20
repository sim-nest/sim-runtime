// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Qualification of the selected module, service, store, and checker surfaces.
//!
//! Each surface names an owner, a responsibility, the surfaces it depends on,
//! and one required scenario. The scenario is a loaded command. A planned
//! disposition, an empty owner, a dependency cycle, two surfaces with one
//! responsibility, or a gap from a required scenario refuses. The conclusion
//! digest binds the rows that conclusion reads. This module does not derive
//! a completion seal.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use sim_kernel::{Datum, Symbol};

use crate::product::{SOURCE_FACT, WorldProduct};
use crate::{
    WorldError, author, conduct, converge, evidence, journal, land, manifest, observe, operate,
    schedule, service, universe, work,
};

/// Reasons the loaded surfaces command returned for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalSurfaces {
    /// Every selected surface is qualified and every required scenario ran.
    pub exact: String,
    /// One selected surface is planned.
    pub planned: String,
    /// One selected surface has an empty owner.
    pub unresolved_owner: String,
    /// The dependency edges contain a cycle.
    pub cycle: String,
    /// Two selected surfaces share one responsibility.
    pub overlap: String,
    /// One required scenario returns a gap.
    pub unimplemented: String,
    /// Digest of the rows the exact conclusion reads.
    pub exact_seal: String,
    /// Digest after one surface is planned.
    pub planned_seal: String,
    /// Digest after one owner is cleared.
    pub unresolved_owner_seal: String,
    /// Digest after one edge closes a cycle.
    pub cycle_seal: String,
    /// Digest after two surfaces share a responsibility.
    pub overlap_seal: String,
    /// Digest after one required scenario returns a gap.
    pub unimplemented_seal: String,
}

/// Runs the selected-surface cases through the loaded surfaces command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalSurfaces, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (exact, exact_seal) = sealed(&command, "exact", "exact")?;
    let (planned, planned_seal) = sealed(&command, "planned", "planned")?;
    let (unresolved_owner, unresolved_owner_seal) =
        sealed(&command, "unresolved-owner", "unresolved-owner")?;
    let (cycle, cycle_seal) = sealed(&command, "cycle", "cycle")?;
    let (overlap, overlap_seal) = sealed(&command, "overlap", "overlap")?;
    let (unimplemented, unimplemented_seal) = sealed(&command, "unimplemented", "unimplemented")?;
    let seals = [
        exact_seal.as_str(),
        planned_seal.as_str(),
        unresolved_owner_seal.as_str(),
        cycle_seal.as_str(),
        overlap_seal.as_str(),
        unimplemented_seal.as_str(),
    ];
    let mut unique = BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalSurfaces {
        exact,
        planned,
        unresolved_owner,
        cycle,
        overlap,
        unimplemented,
        exact_seal,
        planned_seal,
        unresolved_owner_seal,
        cycle_seal,
        overlap_seal,
        unimplemented_seal,
    })
}

/// Digest of the rows one existing surfaces case reads.
///
/// # Errors
///
/// Returns an error when the case is unknown or the digest is not a seal.
pub fn surfaces_digest(case: &str) -> Result<String, WorldError> {
    if !known(case) {
        return Err(WorldError::Qualification(format!(
            "unknown surfaces case {case}"
        )));
    }
    let class = classify(case)?;
    if !hex_seal(&class.digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok(class.digest)
}

/// Runs one named surfaces case.
///
/// # Errors
///
/// Returns an error when a required scenario cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if !known(case) {
        return Ok(node("unknown-case", ""));
    }
    let class = classify(case)?;
    Ok(node(&class.detail, &class.digest))
}

/// Text the conclusion hashed for `case`.
///
/// # Errors
///
/// Returns an error when the case is unknown.
#[cfg(test)]
pub(crate) fn row_report(case: &str) -> Result<String, WorldError> {
    Ok(classify(case)?.report)
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "exact" | "planned" | "unresolved-owner" | "cycle" | "overlap" | "unimplemented"
    )
}

struct Spec {
    id: &'static str,
    owner: &'static str,
    responsibility: &'static str,
    depends: &'static [&'static str],
    scenario: &'static str,
}

macro_rules! row {
    ($id:literal, $owner:literal, $responsibility:literal, [$($dep:literal),*], $scenario:literal) => {
        Spec {
            id: $id,
            owner: $owner,
            responsibility: $responsibility,
            depends: &[$($dep),*],
            scenario: $scenario,
        }
    };
}

const ROWS: &[Spec] = &[
    row!("m1", "projection", "projection", [], "project"),
    row!("m2", "observation", "observation", [], "observe"),
    row!("m3", "source", "source", [], "source"),
    row!("m4", "evidence", "evidence", ["m2"], "evidence"),
    row!("m5", "operation", "operation", ["m10"], "operate"),
    row!("m6", "closure", "closure", ["m2", "m4"], "closure"),
    row!("m7", "control", "control", ["m5"], "control"),
    row!("m8", "work", "work", ["m7"], "work"),
    row!("m9", "authoring", "authoring", [], "author"),
    row!("m10", "journal", "journal", ["m4"], "journal"),
    row!("m11", "drive", "drive", ["m6", "m7"], "drive"),
    row!("m12", "facet", "facet", ["m8", "m10"], "facet"),
    row!(
        "service",
        "model-service",
        "model-service",
        ["m1"],
        "service"
    ),
    row!(
        "store",
        "identity-store",
        "identity-store",
        ["m3", "m10"],
        "store"
    ),
    row!(
        "checker-projection",
        "checker-projection",
        "checker-projection",
        ["m1"],
        "project"
    ),
    row!(
        "checker-observation",
        "checker-observation",
        "checker-observation",
        ["m2"],
        "observe"
    ),
    row!(
        "checker-source",
        "checker-source",
        "checker-source",
        ["m3"],
        "source"
    ),
    row!(
        "checker-evidence",
        "checker-evidence",
        "checker-evidence",
        ["m4"],
        "evidence"
    ),
    row!(
        "checker-operation",
        "checker-operation",
        "checker-operation",
        ["m5"],
        "operate"
    ),
    row!(
        "checker-closure",
        "checker-closure",
        "checker-closure",
        ["m6"],
        "closure"
    ),
    row!(
        "checker-control",
        "checker-control",
        "checker-control",
        ["m7"],
        "control"
    ),
    row!(
        "checker-work",
        "checker-work",
        "checker-work",
        ["m8"],
        "work"
    ),
    row!(
        "checker-authoring",
        "checker-authoring",
        "checker-authoring",
        ["m9"],
        "author"
    ),
    row!(
        "checker-journal",
        "checker-journal",
        "checker-journal",
        ["m10"],
        "journal"
    ),
    row!(
        "checker-drive",
        "checker-drive",
        "checker-drive",
        ["m11"],
        "drive"
    ),
    row!(
        "checker-facet",
        "checker-facet",
        "checker-facet",
        ["m12"],
        "facet"
    ),
    row!(
        "checker-service",
        "checker-service",
        "checker-service",
        ["service"],
        "service"
    ),
    row!(
        "checker-identity",
        "checker-identity",
        "checker-identity",
        ["store"],
        "store"
    ),
];

struct Live {
    id: String,
    owner: String,
    disposition: String,
    responsibility: String,
    depends: Vec<String>,
    scenario: &'static str,
    force_gap: bool,
}

struct Class {
    detail: String,
    digest: String,
    #[cfg(test)]
    report: String,
}

fn classify(case: &str) -> Result<Class, WorldError> {
    let mut rows = selected();
    mutate(&mut rows, case)?;
    let outcomes = outcomes(&rows);
    let mut lines = Vec::with_capacity(rows.len());
    for (row, outcome) in rows.iter().zip(&outcomes) {
        let mut depends = row.depends.clone();
        depends.sort();
        lines.push(format!(
            "{}|owner={}|disposition={}|responsibility={}|depends={}|outcome={}",
            row.id,
            row.owner,
            row.disposition,
            row.responsibility,
            depends.join("+"),
            outcome
        ));
    }
    lines.sort();
    let report = lines.join("\n");
    if report.contains("completion/claim-") {
        return Err(WorldError::Qualification("claim label".to_owned()));
    }
    let detail = if outcomes.iter().any(|item| item.starts_with("gap:")) {
        "unimplemented"
    } else if rows.iter().any(|row| row.disposition == "planned") {
        "planned"
    } else if rows.iter().any(|row| row.owner.is_empty()) {
        "unresolved-owner"
    } else if cyclic(&rows) {
        "cycle"
    } else if overlapped(&rows) {
        "overlap"
    } else {
        "exact"
    };
    let digest = hex(Sha256::digest(report.as_bytes()).as_slice());
    Ok(Class {
        detail: detail.to_owned(),
        digest,
        #[cfg(test)]
        report,
    })
}

fn selected() -> Vec<Live> {
    ROWS.iter()
        .map(|row| Live {
            id: row.id.to_owned(),
            owner: row.owner.to_owned(),
            disposition: "qualified".to_owned(),
            responsibility: row.responsibility.to_owned(),
            depends: row.depends.iter().map(|item| (*item).to_owned()).collect(),
            scenario: row.scenario,
            force_gap: false,
        })
        .collect()
}

fn mutate(rows: &mut [Live], case: &str) -> Result<(), WorldError> {
    match case {
        "exact" => Ok(()),
        "planned" => {
            row_mut(rows, "m12")?.disposition = "planned".to_owned();
            Ok(())
        }
        "unresolved-owner" => {
            row_mut(rows, "store")?.owner.clear();
            Ok(())
        }
        "cycle" => {
            row_mut(rows, "m2")?.depends.push("m6".to_owned());
            Ok(())
        }
        "overlap" => {
            row_mut(rows, "checker-projection")?.responsibility = "projection".to_owned();
            Ok(())
        }
        "unimplemented" => {
            row_mut(rows, "checker-identity")?.force_gap = true;
            Ok(())
        }
        _ => Err(WorldError::Qualification(format!(
            "unknown surfaces case {case}"
        ))),
    }
}

fn row_mut<'a>(rows: &'a mut [Live], id: &str) -> Result<&'a mut Live, WorldError> {
    rows.iter_mut()
        .find(|row| row.id == id)
        .ok_or_else(|| WorldError::Qualification(format!("missing surface {id}")))
}

fn outcomes(rows: &[Live]) -> Vec<String> {
    let mut cache = BTreeMap::<&str, String>::new();
    let mut found = Vec::with_capacity(rows.len());
    for row in rows {
        if row.force_gap {
            found.push(tag_of(
                &gap_scenario(row.scenario).unwrap_or_else(error_datum),
            ));
            continue;
        }
        if let Some(saved) = cache.get(row.scenario) {
            found.push(saved.clone());
            continue;
        }
        let outcome = tag_of(&invoke(row.scenario).unwrap_or_else(error_datum));
        cache.insert(row.scenario, outcome.clone());
        found.push(outcome);
    }
    found
}

fn error_datum(error: WorldError) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "surfaces-gap-v1"),
        fields: vec![(
            Symbol::new("reason"),
            Datum::String(error.to_string().chars().take(160).collect()),
        )],
    }
}

fn invoke(scenario: &str) -> Result<Datum, WorldError> {
    match scenario {
        "project" => Ok(WorldProduct::bundled()?
            .project(
                "world/public-api-v1",
                SOURCE_FACT,
                Datum::String("api-v1".to_owned()),
                None,
            )?
            .value),
        "observe" => observe::snapshot("model", "ci/run"),
        "source" => universe::run("exact"),
        "evidence" => evidence::run("admit"),
        "operate" => operate::run("measure"),
        "closure" => converge::run("preserve"),
        "control" => conduct::run("quiet"),
        "work" => work::run("admitted"),
        "author" => author::run("selected"),
        "journal" => journal::run("replay"),
        "drive" => schedule::run("frontier"),
        "facet" => land::run("status"),
        "service" => service::snapshot("model", "registry/checksum/sim-kernel"),
        "store" => manifest::run("owner"),
        other => Err(WorldError::Qualification(format!(
            "unknown scenario {other}"
        ))),
    }
}

fn gap_scenario(scenario: &str) -> Result<Datum, WorldError> {
    match scenario {
        "project" => WorldProduct::bundled()?
            .project(
                "not-a-kind",
                SOURCE_FACT,
                Datum::String("api-v1".to_owned()),
                None,
            )
            .map(|projection| projection.value),
        "observe" => observe::snapshot("not-a-batch", "ci/run"),
        "source" => universe::run("not-a-case"),
        "evidence" => evidence::run("not-a-case"),
        "operate" => operate::run("not-a-case"),
        "closure" => converge::run("not-a-case"),
        "control" => conduct::run("not-a-case"),
        "work" => work::run("not-a-case"),
        "author" => author::run("not-a-case"),
        "journal" => journal::run("not-a-case"),
        "drive" => schedule::run("not-a-case"),
        "facet" => land::run("not-a-case"),
        "service" => service::snapshot("not-a-realization", "registry/checksum/sim-kernel"),
        "store" => manifest::run("not-a-case"),
        other => Err(WorldError::Qualification(format!(
            "unknown scenario {other}"
        ))),
    }
}

fn tag_of(datum: &Datum) -> String {
    match datum {
        Datum::Node { tag, .. } => {
            let name = tag.as_qualified_str();
            if name.contains("-gap-") {
                format!("gap:{name}")
            } else {
                format!("ok:{name}")
            }
        }
        _ => "gap:not-node".to_owned(),
    }
}

fn cyclic(rows: &[Live]) -> bool {
    let index: BTreeMap<&str, &Live> = rows.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut color = BTreeMap::<&str, u8>::new();
    rows.iter().any(|row| visit(&row.id, &index, &mut color))
}

fn visit<'a>(
    id: &'a str,
    index: &BTreeMap<&'a str, &'a Live>,
    color: &mut BTreeMap<&'a str, u8>,
) -> bool {
    match color.get(id).copied() {
        Some(1) => return true,
        Some(2) => return false,
        _ => {}
    }
    let Some(row) = index.get(id) else {
        return true;
    };
    color.insert(id, 1);
    for dep in &row.depends {
        if visit(dep, index, color) {
            return true;
        }
    }
    color.insert(id, 2);
    false
}

fn overlapped(rows: &[Live]) -> bool {
    let mut seen = BTreeSet::new();
    rows.iter()
        .any(|row| !seen.insert(row.responsibility.as_str()))
}

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "surfaces".to_owned(), case.to_owned()])?;
    let found =
        detail(&datum).ok_or_else(|| WorldError::Qualification(format!("{case} unreadable")))?;
    if found != intended {
        return Err(WorldError::Qualification(format!(
            "{case} returned {found}"
        )));
    }
    if found.contains("completion/claim-") {
        return Err(WorldError::Qualification(format!(
            "{case} carried a claim label"
        )));
    }
    let digest = text(&datum, "world/surfaces-v1", "digest").unwrap_or_default();
    if !hex_seal(&digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok((found, digest))
}

fn hex_seal(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        && value.bytes().any(|byte| byte != b'0')
}

fn node(detail: &str, digest: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "surfaces-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/surfaces-v1", "detail")
}

fn text(value: &Datum, tag: &str, name: &str) -> Option<String> {
    let Datum::Node { tag: found, fields } = value else {
        return None;
    };
    if found.as_qualified_str() != tag {
        return None;
    }
    fields.iter().find_map(|(field, item)| {
        if field.to_string() != name {
            return None;
        }
        match item {
            Datum::String(text) => Some(text.clone()),
            _ => None,
        }
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{execute, run};
    use crate::command::WorldCommandLib;

    #[test]
    fn selected_surfaces_qualify_and_each_fault_stands_alone() {
        let exact_rows = super::row_report("exact").expect("rows");
        assert!(!exact_rows.contains("gap:"), "{exact_rows}");
        assert!(exact_rows.contains("ok:"), "{exact_rows}");
        let unimplemented_rows = super::row_report("unimplemented").expect("gap rows");
        assert!(unimplemented_rows.contains("gap:"), "{unimplemented_rows}");
        let found = execute().expect("surfaces");
        assert_eq!(found.exact, "exact");
        assert_eq!(found.planned, "planned");
        assert_eq!(found.unresolved_owner, "unresolved-owner");
        assert_eq!(found.cycle, "cycle");
        assert_eq!(found.overlap, "overlap");
        assert_eq!(found.unimplemented, "unimplemented");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("exact", found.exact_seal.as_str()),
            ("planned", found.planned_seal.as_str()),
            ("unresolved-owner", found.unresolved_owner_seal.as_str()),
            ("cycle", found.cycle_seal.as_str()),
            ("overlap", found.overlap_seal.as_str()),
            ("unimplemented", found.unimplemented_seal.as_str()),
        ] {
            assert_eq!(super::surfaces_digest(case).expect(case), seal, "{case}");
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_a_surface_case() {
        let found = run("completion/claim-2").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    #[test]
    fn a_succession_record_is_not_a_surface_case() {
        let found = run("ownership.succession-record").expect("record");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "surfaces".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/surfaces-v1", "digest").unwrap_or_default()
    }
}
