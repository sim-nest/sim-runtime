// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh-succession gate for the portable phase union.
//!
//! The leaf calls the loaded fresh command. An independent change on
//! the foreign project admits. The construction specimen stays open.
//! The record does not derive a completion seal.

use std::collections::BTreeSet;

use sim_kernel::{Datum, Export, Lib};
use sim_lib_roadmap_runner::ProofLeaf;

use crate::WorldError;
use crate::command::WorldCommandLib;

pub(super) const ENTRY: &str = "cli/main/world";

const RECORD: &str = "world/fresh-v1";
const GAP: &str = "world/fresh-gap-v1";

pub(super) fn fresh_leaf() -> Result<ProofLeaf, WorldError> {
    let command = super::project::resident_command()?;
    Ok(leaf_from(capture(&command)?))
}

fn capture(command: &WorldCommandLib) -> Result<Capture, WorldError> {
    let exports = command.manifest().exports.iter().any(|export| {
        matches!(
            export,
            Export::Function { symbol, .. } if symbol.to_string() == ENTRY
        )
    });
    let admitted = call(command, &export("harbor"), INDEPENDENT)?;
    let specimen = call(command, &export("harbor"), SPECIMEN)?;
    let warm = call(
        command,
        &format!("{}checkout\n", export("harbor")),
        INDEPENDENT,
    )?;
    let linux = call(
        command,
        &format!("{}claim reboot\n", export("harbor")),
        INDEPENDENT,
    )?;
    Ok(Capture {
        admitted,
        specimen,
        warm,
        linux,
        exports,
        again: call(command, &export("harbor"), INDEPENDENT)?,
    })
}

fn call(command: &WorldCommandLib, export_text: &str, change: &str) -> Result<Datum, WorldError> {
    command.run(&[
        "sim".to_owned(),
        "fresh".to_owned(),
        export_text.to_owned(),
        change.to_owned(),
    ])
}

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

const INDEPENDENT: &str = "independent harbor provider/marker observed marker-next";
const SPECIMEN: &str = "shade-1 harbor provider/shade observed shade-next";

struct Capture {
    admitted: Datum,
    specimen: Datum,
    warm: Datum,
    linux: Datum,
    exports: bool,
    again: Datum,
}

fn leaf_from(capture: Capture) -> ProofLeaf {
    ProofLeaf::SourceDeckPredicate {
        name: "phase/28".to_owned(),
        actual_deck: actual_deck(&capture),
        expected_deck: expected_deck(),
        required_claims: required_claims(),
        present_claims: present_claims(&capture),
    }
}

fn actual_deck(capture: &Capture) -> String {
    let entry = if capture.exports { ENTRY } else { "missing" };
    [
        format!("entry {entry}"),
        format!("admitted {}", line(&capture.admitted)),
        format!("specimen {}", reason(&capture.specimen)),
        format!("warm {}", reason(&capture.warm)),
        format!("linux {}", reason(&capture.linux)),
        format!("repeat {}", repeat(capture)),
    ]
    .join("\n")
}

fn expected_deck() -> String {
    [
        format!("entry {ENTRY}"),
        "admitted succession portable seal unclaimed converge-second 0 deliver-second 0".to_owned(),
        "specimen construction-specimen".to_owned(),
        "warm warm-state".to_owned(),
        "linux linux-claim".to_owned(),
        "repeat match".to_owned(),
    ]
    .join("\n")
}

fn required_claims() -> BTreeSet<String> {
    BTreeSet::from([
        "fresh succession portable seal unclaimed".to_owned(),
        "fresh specimen construction-specimen".to_owned(),
    ])
}

fn present_claims(capture: &Capture) -> BTreeSet<String> {
    let mut claims = BTreeSet::new();
    if line(&capture.admitted).contains("seal unclaimed")
        && line(&capture.admitted).contains("succession portable")
    {
        claims.insert("fresh succession portable seal unclaimed".to_owned());
    }
    if reason(&capture.specimen) == "construction-specimen" {
        claims.insert("fresh specimen construction-specimen".to_owned());
    }
    claims
}

fn line(value: &Datum) -> String {
    let Some(fields) = fields(value, RECORD) else {
        return format!("gap {}", reason(value));
    };
    format!(
        "succession {} seal {} converge-second {} deliver-second {}",
        string_field(fields, "succession"),
        string_field(fields, "seal"),
        number_field(fields, "converge-second"),
        number_field(fields, "deliver-second")
    )
}

fn repeat(capture: &Capture) -> &'static str {
    if capture.admitted == capture.again {
        "match"
    } else {
        "drift"
    }
}

fn reason(value: &Datum) -> String {
    fields(value, GAP)
        .and_then(|fields| match field(fields, "reason") {
            Some(Datum::String(reason)) => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "missing".to_owned())
}

fn fields<'a>(value: &'a Datum, tag: &str) -> Option<&'a [(sim_kernel::Symbol, Datum)]> {
    match value {
        Datum::Node { tag: found, fields } if found.as_qualified_str() == tag => Some(fields),
        _ => None,
    }
}

fn field<'a>(fields: &'a [(sim_kernel::Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

fn string_field(fields: &[(sim_kernel::Symbol, Datum)], name: &str) -> String {
    match field(fields, name) {
        Some(Datum::String(value)) => value.clone(),
        _ => "missing".to_owned(),
    }
}

fn number_field(fields: &[(sim_kernel::Symbol, Datum)], name: &str) -> String {
    match field(fields, name) {
        Some(Datum::Number(value)) => value.canonical.clone(),
        _ => "missing".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use sim_lib_roadmap_runner::{
        ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, SandboxAttempt,
        SandboxLauncher, SandboxRefusal, SandboxRequest, execute_proof_leaf,
    };

    use super::*;
    use crate::command::WorldCommandLib;

    struct RefuseLaunch;

    impl SandboxLauncher for RefuseLaunch {
        fn id(&self) -> &str {
            "fresh-gate-refuse"
        }

        fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            SandboxAttempt::Refused(SandboxRefusal {
                launcher: self.id().into(),
                reason: "fresh gate has no owner command".into(),
                report: None,
            })
        }
    }

    fn disposition(leaf: ProofLeaf) -> ProofDisposition {
        let name = leaf.name().to_owned();
        let catalog = ProofCatalog::new([leaf]).expect("catalog");
        let receipt = execute_proof_leaf(
            &catalog,
            &name,
            &RefuseLaunch,
            &ProcessCancellation::default(),
            "fresh-gate",
        )
        .expect("leaf");
        receipt.disposition
    }

    #[test]
    fn the_loaded_fresh_command_refuses_the_construction_specimen() {
        let command = WorldCommandLib::new().expect("command");
        let captured = capture(&command).expect("capture");
        let leaf = leaf_from(captured);
        let ProofLeaf::SourceDeckPredicate {
            name,
            actual_deck,
            expected_deck,
            ..
        } = &leaf
        else {
            panic!("fresh gate is a source-deck predicate");
        };
        assert_eq!(name, "phase/28");
        assert_eq!(actual_deck, expected_deck);
        assert!(actual_deck.contains("seal unclaimed"));
        assert!(actual_deck.contains("specimen construction-specimen"));
        assert!(!actual_deck.contains("derive_completion"));
        let source = include_str!("fresh.rs");
        assert!(!source.contains("derive_completion"));
        assert!(!source.contains("PortableClosed"));
        assert_eq!(disposition(leaf), ProofDisposition::Passed);
    }
}
