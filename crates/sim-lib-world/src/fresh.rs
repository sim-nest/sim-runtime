// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh succession over one exported model record.
//!
//! The caller supplies the export and one later change. A construction
//! specimen stays open even when one succession record closes. The
//! record does not derive a completion seal.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::succeed;

const CONSTRUCTION_ID: &str = "shade-1";

/// Admits one later change that is not the construction specimen.
///
/// # Errors
///
/// Returns an error when the succession record is not a node the
/// command can read.
pub(crate) fn run(export_text: &str, change_text: &str) -> Result<Datum, WorldError> {
    if construction_id(change_text) {
        return Ok(gap("construction-specimen"));
    }
    admit(succeed::run(export_text, change_text)?)
}

fn construction_id(change_text: &str) -> bool {
    change_text.split_whitespace().next() == Some(CONSTRUCTION_ID)
}

fn admit(record: Datum) -> Result<Datum, WorldError> {
    let Datum::Node { tag, fields } = &record else {
        return Err(WorldError::Qualification(
            "fresh record is not a node".to_owned(),
        ));
    };
    let qualified_tag = tag.as_qualified_str();
    match qualified_tag.as_str() {
        "world/succession-gap-v1" => Ok(gap(&string_field(fields, "reason").unwrap_or_default())),
        "world/succession-v1" if qualified(fields) => Ok(admitted(fields)),
        "world/succession-v1" => Ok(gap("succession-open")),
        other => Err(WorldError::Qualification(format!("fresh record {other}"))),
    }
}

fn qualified(fields: &[(Symbol, Datum)]) -> bool {
    string_field(fields, "closed").as_deref() == Some("portable")
        && number_field(fields, "converge-second") == Some(0)
        && number_field(fields, "deliver-second") == Some(0)
        && number_field(fields, "holds") == Some(0)
        && number_field(fields, "effects") == Some(0)
        && number_field(fields, "subprocesses") == Some(0)
        && string_field(fields, "linux").as_deref() == Some("absent")
        && string_field(fields, "recovery").as_deref() == Some("match")
        && bool_field(fields, "resident") == Some(true)
}

fn admitted(fields: &[(Symbol, Datum)]) -> Datum {
    node(
        "fresh-v1",
        vec![
            (
                "change",
                Datum::String(string_field(fields, "change").unwrap_or_default()),
            ),
            ("converge-second", crate::query::count_datum(0)),
            ("deliver-second", crate::query::count_datum(0)),
            ("effects", crate::query::count_datum(0)),
            ("linux", Datum::String("absent".to_owned())),
            (
                "project",
                Datum::String(string_field(fields, "project").unwrap_or_default()),
            ),
            ("resident", Datum::Bool(true)),
            ("seal", Datum::String("unclaimed".to_owned())),
            ("subprocesses", crate::query::count_datum(0)),
            ("succession", Datum::String("portable".to_owned())),
        ],
    )
}

fn gap(reason: &str) -> Datum {
    node(
        "fresh-gap-v1",
        vec![
            ("effects", crate::query::count_datum(0)),
            ("reason", Datum::String(reason.to_owned())),
            ("resident", Datum::Bool(true)),
            ("seal", Datum::String("unclaimed".to_owned())),
            ("subprocesses", crate::query::count_datum(0)),
        ],
    )
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

fn string_field(fields: &[(Symbol, Datum)], name: &str) -> Option<String> {
    match field(fields, name) {
        Some(Datum::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn number_field(fields: &[(Symbol, Datum)], name: &str) -> Option<u64> {
    match field(fields, name) {
        Some(Datum::Number(value)) => value.canonical.parse().ok(),
        _ => None,
    }
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> Option<bool> {
    match field(fields, name) {
        Some(Datum::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(project: &str) -> String {
        format!(
            "\
export world/succession-export-v1
package sim-incremental-core 0.5.2 registry
package sim-storage-port 0.2.2 registry
package sim-lib-world 0.1.1 registry
project {project}
input provider/widget observed widget-body
input remote/hostile unavailable -
"
        )
    }

    fn rows<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
        match value {
            Datum::Node { tag: found, fields } if found.as_qualified_str() == tag => fields,
            other => panic!("expected {tag}, got {other:?}"),
        }
    }

    fn text(value: &Datum, tag: &str, name: &str) -> String {
        string_field(rows(value, tag), name).unwrap_or_else(|| panic!("missing {name}"))
    }

    #[test]
    fn the_construction_specimen_stays_open_when_one_succession_closes() {
        let export = export("lantern");
        let change = "shade-1 lantern provider/shade observed shade-next";
        let succeeded = succeed::run(&export, change).expect("succession");
        assert_eq!(
            text(&succeeded, "world/succession-v1", "closed"),
            "portable"
        );
        let fresh = run(&export, change).expect("fresh");
        assert_eq!(
            text(&fresh, "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        assert_eq!(text(&fresh, "world/fresh-gap-v1", "seal"), "unclaimed");
    }

    #[test]
    fn an_independent_change_admits_without_a_completion_seal() {
        let export = export("harbor");
        let change = "independent harbor provider/marker observed marker-next";
        let first = run(&export, change).expect("fresh");
        let second = run(&export, change).expect("fresh");
        assert_eq!(first, second);
        let fields = rows(&first, "world/fresh-v1");
        assert_eq!(
            string_field(fields, "succession").as_deref(),
            Some("portable")
        );
        assert_eq!(string_field(fields, "seal").as_deref(), Some("unclaimed"));
        assert_eq!(string_field(fields, "project").as_deref(), Some("harbor"));
        assert_eq!(number_field(fields, "converge-second"), Some(0));
        assert_eq!(number_field(fields, "deliver-second"), Some(0));
        assert_eq!(string_field(fields, "linux").as_deref(), Some("absent"));
    }

    #[test]
    fn warm_linux_and_private_inputs_stay_open() {
        let warm = run(
            &format!("{}checkout\n", export("harbor")),
            "independent harbor provider/marker observed marker-next",
        )
        .expect("warm");
        assert_eq!(text(&warm, "world/fresh-gap-v1", "reason"), "warm-state");
        let linux = run(
            &format!("{}claim reboot\n", export("harbor")),
            "independent harbor provider/marker observed marker-next",
        )
        .expect("linux");
        assert_eq!(text(&linux, "world/fresh-gap-v1", "reason"), "linux-claim");
        let private = run(
            &export("harbor"),
            "independent harbor provider/marker observed private-material:s3cret-token",
        )
        .expect("private");
        let rendered = format!("{private:?}");
        assert!(!rendered.contains("s3cret-token"));
        assert_eq!(
            text(&private, "world/fresh-gap-v1", "reason"),
            "private-material"
        );
    }
}
