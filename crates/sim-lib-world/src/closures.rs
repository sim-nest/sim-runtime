// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Production and conformance closures, with retained facts kept outside them.
//!
//! Each selected member names its closure, its kind, and, for a live member,
//! the loaded command that member runs. A v3 tag from that command, a present
//! executable, reader, schema, fixture oracle, or parser, or a retained fact
//! placed in production refuses. The conclusion digest binds the rows that
//! conclusion reads. This module does not derive a completion seal.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use sim_kernel::{Datum, Symbol};

use crate::product::{SOURCE_FACT, WorldProduct};
use crate::{WorldError, evidence, journal, universe};

/// Reasons the loaded closures command returned for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalClosures {
    /// Neither closure contains a forbidden member, and retained facts stay out.
    pub closed: String,
    /// One v3 executable is present.
    pub executable: String,
    /// One v3 reader is present.
    pub reader: String,
    /// One v3 schema is present.
    pub schema: String,
    /// One fixture oracle is present.
    pub oracle: String,
    /// One v3 parser is present.
    pub parser: String,
    /// One retained fact is used as production input.
    pub live_fact: String,
    /// Digest of the rows the closed conclusion reads.
    pub closed_seal: String,
    /// Digest after one executable is present.
    pub executable_seal: String,
    /// Digest after one reader is present.
    pub reader_seal: String,
    /// Digest after one schema is present.
    pub schema_seal: String,
    /// Digest after one fixture oracle is present.
    pub oracle_seal: String,
    /// Digest after one parser is present.
    pub parser_seal: String,
    /// Digest after one retained fact enters production.
    pub live_fact_seal: String,
}

/// Runs the selected closure cases through the loaded closures command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalClosures, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (closed, closed_seal) = sealed(&command, "closed", "closed")?;
    let (executable, executable_seal) = sealed(&command, "executable", "executable")?;
    let (reader, reader_seal) = sealed(&command, "reader", "reader")?;
    let (schema, schema_seal) = sealed(&command, "schema", "schema")?;
    let (oracle, oracle_seal) = sealed(&command, "oracle", "oracle")?;
    let (parser, parser_seal) = sealed(&command, "parser", "parser")?;
    let (live_fact, live_fact_seal) = sealed(&command, "live-fact", "live-fact")?;
    let seals = [
        closed_seal.as_str(),
        executable_seal.as_str(),
        reader_seal.as_str(),
        schema_seal.as_str(),
        oracle_seal.as_str(),
        parser_seal.as_str(),
        live_fact_seal.as_str(),
    ];
    let mut unique = BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalClosures {
        closed,
        executable,
        reader,
        schema,
        oracle,
        parser,
        live_fact,
        closed_seal,
        executable_seal,
        reader_seal,
        schema_seal,
        oracle_seal,
        parser_seal,
        live_fact_seal,
    })
}

/// Digest of the rows one existing closures case reads.
///
/// # Errors
///
/// Returns an error when the case is unknown or the digest is not a seal.
pub fn closures_digest(case: &str) -> Result<String, WorldError> {
    if !known(case) {
        return Err(WorldError::Qualification(format!(
            "unknown closures case {case}"
        )));
    }
    let class = classify(case)?;
    if !hex_seal(&class.digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok(class.digest)
}

/// Runs one named closures case.
///
/// # Errors
///
/// Returns an error when a live member cannot be represented.
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
        "closed" | "executable" | "reader" | "schema" | "oracle" | "parser" | "live-fact"
    )
}

struct Spec {
    id: &'static str,
    closure: &'static str,
    kind: &'static str,
    scenario: &'static str,
}

macro_rules! row {
    ($id:literal, $closure:literal, $kind:literal, $scenario:literal) => {
        Spec {
            id: $id,
            closure: $closure,
            kind: $kind,
            scenario: $scenario,
        }
    };
}

const ROWS: &[Spec] = &[
    row!("production-projection", "production", "product", "project"),
    row!("production-journal", "production", "product", "journal"),
    row!("conformance-source", "conformance", "product", "source"),
    row!("conformance-evidence", "conformance", "product", "evidence"),
    row!("production-executable", "production", "executable", ""),
    row!("production-reader", "production", "reader", ""),
    row!("production-schema", "production", "schema", ""),
    row!("production-oracle", "production", "fixture-oracle", ""),
    row!("production-parser", "production", "parser", ""),
    row!("conformance-executable", "conformance", "executable", ""),
    row!("conformance-reader", "conformance", "reader", ""),
    row!("conformance-schema", "conformance", "schema", ""),
    row!("conformance-oracle", "conformance", "fixture-oracle", ""),
    row!("conformance-parser", "conformance", "parser", ""),
    row!("retained-sup20-journal", "retained", "fact", ""),
    row!("retained-sup20-contracts", "retained", "fact", ""),
];

struct Live {
    id: String,
    closure: String,
    kind: String,
    disposition: String,
    scenario: &'static str,
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
        lines.push(format!(
            "{}|closure={}|kind={}|disposition={}|outcome={}",
            row.id, row.closure, row.kind, row.disposition, outcome
        ));
    }
    lines.sort();
    let report = lines.join("\n");
    if report.contains("completion/claim-") {
        return Err(WorldError::Qualification("claim label".to_owned()));
    }
    let detail = detail_of(&rows, &outcomes);
    let digest = hex(Sha256::digest(report.as_bytes()).as_slice());
    Ok(Class {
        detail: detail.to_owned(),
        digest,
        #[cfg(test)]
        report,
    })
}

fn detail_of(rows: &[Live], outcomes: &[String]) -> &'static str {
    if outcomes.iter().any(|item| item.contains("v3")) {
        return "executable";
    }
    if present(rows, "executable") {
        return "executable";
    }
    if present(rows, "reader") {
        return "reader";
    }
    if present(rows, "schema") {
        return "schema";
    }
    if present(rows, "fixture-oracle") {
        return "oracle";
    }
    if present(rows, "parser") {
        return "parser";
    }
    if rows
        .iter()
        .any(|row| row.kind == "fact" && row.disposition == "production")
    {
        return "live-fact";
    }
    if outcomes.iter().any(|item| item.starts_with("gap:")) {
        return "gap";
    }
    "closed"
}

fn present(rows: &[Live], kind: &str) -> bool {
    rows.iter()
        .any(|row| row.kind == kind && row.disposition == "present")
}

fn selected() -> Vec<Live> {
    ROWS.iter()
        .map(|row| Live {
            id: row.id.to_owned(),
            closure: row.closure.to_owned(),
            kind: row.kind.to_owned(),
            disposition: default_disposition(row).to_owned(),
            scenario: row.scenario,
        })
        .collect()
}

fn default_disposition(row: &Spec) -> &'static str {
    if !row.scenario.is_empty() {
        "current"
    } else if row.kind == "fact" {
        "historical"
    } else {
        "absent"
    }
}

fn mutate(rows: &mut [Live], case: &str) -> Result<(), WorldError> {
    match case {
        "closed" => Ok(()),
        "executable" => {
            row_mut(rows, "production-executable")?.disposition = "present".to_owned();
            Ok(())
        }
        "reader" => {
            row_mut(rows, "conformance-reader")?.disposition = "present".to_owned();
            Ok(())
        }
        "schema" => {
            row_mut(rows, "production-schema")?.disposition = "present".to_owned();
            Ok(())
        }
        "oracle" => {
            row_mut(rows, "conformance-oracle")?.disposition = "present".to_owned();
            Ok(())
        }
        "parser" => {
            row_mut(rows, "production-parser")?.disposition = "present".to_owned();
            Ok(())
        }
        "live-fact" => {
            row_mut(rows, "retained-sup20-journal")?.disposition = "production".to_owned();
            Ok(())
        }
        _ => Err(WorldError::Qualification(format!(
            "unknown closures case {case}"
        ))),
    }
}

fn row_mut<'a>(rows: &'a mut [Live], id: &str) -> Result<&'a mut Live, WorldError> {
    rows.iter_mut()
        .find(|row| row.id == id)
        .ok_or_else(|| WorldError::Qualification(format!("missing closure member {id}")))
}

fn outcomes(rows: &[Live]) -> Vec<String> {
    let mut found = Vec::with_capacity(rows.len());
    for row in rows {
        if row.scenario.is_empty() {
            found.push(row.disposition.clone());
            continue;
        }
        found.push(tag_of(&invoke(row.scenario).unwrap_or_else(error_datum)));
    }
    found
}

fn error_datum(error: WorldError) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "closures-gap-v1"),
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
        "journal" => journal::run("replay"),
        "source" => universe::run("exact"),
        "evidence" => evidence::run("admit"),
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

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "closures".to_owned(), case.to_owned()])?;
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
    let digest = text(&datum, "world/closures-v1", "digest").unwrap_or_default();
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
        tag: Symbol::qualified("world", "closures-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/closures-v1", "detail")
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
    fn selected_closures_hold_and_each_fault_stands_alone() {
        let closed_rows = super::row_report("closed").expect("rows");
        assert!(closed_rows.contains("closure=production"), "{closed_rows}");
        assert!(closed_rows.contains("closure=conformance"), "{closed_rows}");
        assert!(
            closed_rows.contains(
                "retained-sup20-journal|closure=retained|kind=fact|disposition=historical"
            ),
            "{closed_rows}"
        );
        assert!(
            !closed_rows.contains("disposition=present"),
            "{closed_rows}"
        );
        assert!(!closed_rows.contains("v3"), "{closed_rows}");
        let live = super::row_report("live-fact").expect("live fact");
        assert!(
            live.contains(
                "retained-sup20-journal|closure=retained|kind=fact|disposition=production"
            ),
            "{live}"
        );
        let found = execute().expect("closures");
        assert_eq!(found.closed, "closed");
        assert_eq!(found.executable, "executable");
        assert_eq!(found.reader, "reader");
        assert_eq!(found.schema, "schema");
        assert_eq!(found.oracle, "oracle");
        assert_eq!(found.parser, "parser");
        assert_eq!(found.live_fact, "live-fact");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("closed", found.closed_seal.as_str()),
            ("executable", found.executable_seal.as_str()),
            ("reader", found.reader_seal.as_str()),
            ("schema", found.schema_seal.as_str()),
            ("oracle", found.oracle_seal.as_str()),
            ("parser", found.parser_seal.as_str()),
            ("live-fact", found.live_fact_seal.as_str()),
        ] {
            assert_eq!(super::closures_digest(case).expect(case), seal, "{case}");
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_a_closure_case() {
        let found = run("completion/claim-3").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    #[test]
    fn a_retirement_guard_is_not_a_closure_case() {
        let found = run("retirement-guard").expect("guard");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "closures".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/closures-v1", "digest").unwrap_or_default()
    }
}
