// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Final source universe and the conclusion seal that reads it.
//!
//! The loaded source command seals the captured resolver document and the
//! same document with one required class removed. A final universe has a
//! target, a compiling resolver, a verified checksum, a feature, resolver
//! and lock configuration, a toolchain release, and a nested input. The
//! conclusion seal binds the facts and policies that conclusion reads. An
//! unread policy stays out of that seal. A fact the conclusion reads and
//! the universe does not contain is unbound. This module does not derive a
//! completion seal.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::command::WorldCommandLib;
use crate::seal::{self, inspect_final};

/// Reasons the loaded source command returned for one final universe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalUniverse {
    /// Captured document has every required class and reads every sealed fact.
    pub complete: String,
    /// The captured document with its target line removed.
    pub missing_target: String,
    /// The captured document with its resolver domains removed.
    pub missing_resolver: String,
    /// The captured document with member checksums cleared.
    pub missing_checksum: String,
    /// The captured document with its feature lines removed.
    pub missing_feature: String,
    /// The captured document with its configuration lines removed.
    pub missing_config: String,
    /// The captured document with its toolchain lines removed.
    pub missing_toolchain: String,
    /// The captured document with generated rows and nested dependencies removed.
    pub missing_nested: String,
    /// A present policy the conclusion does not read stays out of the seal.
    pub unread_policy: String,
    /// A fact the conclusion reads is not in the sealed universe.
    pub unbound_fact: String,
    /// The conclusion reads one present fact and one present policy, and no other policy.
    pub exact: String,
}

/// Runs the final-universe cases through the loaded source command.
///
/// # Errors
///
/// Returns an error when the command cannot run or a case returns a reason
/// other than the one that case exists to show.
pub fn execute() -> Result<FinalUniverse, WorldError> {
    let command = WorldCommandLib::new()?;
    Ok(FinalUniverse {
        complete: reason(&command, "complete", "complete")?,
        missing_target: reason(&command, "missing-target", "missing-target")?,
        missing_resolver: reason(&command, "missing-resolver", "missing-resolver")?,
        missing_checksum: reason(&command, "missing-checksum", "missing-checksum")?,
        missing_feature: reason(&command, "missing-feature", "missing-feature")?,
        missing_config: reason(&command, "missing-config", "missing-config")?,
        missing_toolchain: reason(&command, "missing-toolchain", "missing-toolchain")?,
        missing_nested: reason(&command, "missing-nested", "missing-nested")?,
        unread_policy: reason(&command, "unread-policy", "unread-policy")?,
        unbound_fact: reason(&command, "unbound-fact", "unbound-fact")?,
        exact: reason(&command, "exact", "exact")?,
    })
}

/// Runs one named source case through the sealed captured document.
///
/// # Errors
///
/// Returns an error when the captured document cannot be sealed.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if !known(case) {
        return Ok(node("unknown-case", ""));
    }
    let class = classify(case)?;
    Ok(node(&class.detail, &class.digest))
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "complete"
            | "missing-target"
            | "missing-resolver"
            | "missing-checksum"
            | "missing-feature"
            | "missing-config"
            | "missing-toolchain"
            | "missing-nested"
            | "unread-policy"
            | "unbound-fact"
            | "exact"
    )
}

struct Class {
    detail: String,
    digest: String,
}

fn classify(case: &str) -> Result<Class, WorldError> {
    let document = document(case)?;
    let face = inspect_final(document.as_deref())?;
    Ok(class_of(&face))
}

fn class_of(face: &seal::FinalFace) -> Class {
    if let Some(reason) = face.early {
        return token(reason);
    }
    if face.target.is_empty() {
        return token("missing-target");
    }
    if face.compiling_domains == 0 {
        return token("missing-resolver");
    }
    if face.verified_checksums == 0 {
        return token("missing-checksum");
    }
    if face.features == 0 {
        return token("missing-feature");
    }
    if !face.config_resolver || !face.config_lock {
        return token("missing-config");
    }
    if face.toolchain_release.is_empty() || face.toolchain_release == "0" {
        return token("missing-toolchain");
    }
    if !face.nested {
        return token("missing-nested");
    }
    let explicit = !face.reads_facts.is_empty() || !face.reads_policies.is_empty();
    let facts: BTreeSet<String> = if explicit {
        face.reads_facts.clone()
    } else {
        face.marks.keys().cloned().collect()
    };
    let policies: BTreeSet<String> = if explicit {
        face.reads_policies.clone()
    } else {
        face.policies.keys().cloned().collect()
    };
    if facts.iter().any(|fact| !face.marks.contains_key(fact)) {
        return token("unbound-fact");
    }
    if policies
        .iter()
        .any(|policy| !face.policies.contains_key(policy))
    {
        return token("unbound-policy");
    }
    let mut lines = Vec::new();
    for fact in &facts {
        lines.push(format!("fact|{fact}={}", face.marks[fact]));
    }
    for policy in &policies {
        lines.push(format!("policy|{policy}={}", face.policies[policy]));
    }
    lines.sort();
    let unread = face.policies.keys().any(|name| !policies.contains(name));
    if lines.iter().any(|line| {
        face.policies
            .keys()
            .any(|name| !policies.contains(name) && line.starts_with(&format!("policy|{name}=")))
    }) {
        return token("leaked-policy");
    }
    if face.admission != "admit" {
        let reason = if face.refusal.is_empty() {
            "seal-refused"
        } else {
            face.refusal.as_str()
        };
        return Class {
            detail: reason.to_owned(),
            digest: digest_of(&lines),
        };
    }
    let detail = if !explicit && !unread {
        "complete"
    } else if explicit && unread {
        "unread-policy"
    } else if explicit {
        "exact"
    } else {
        "seal-refused"
    };
    Class {
        detail: detail.to_owned(),
        digest: digest_of(&lines),
    }
}

fn token(detail: &str) -> Class {
    Class {
        detail: detail.to_owned(),
        digest: String::new(),
    }
}

fn document(case: &str) -> Result<Option<String>, WorldError> {
    let base = crate::captured_inputs();
    let text = match case {
        "complete" => return Ok(None),
        "missing-target" => drop_prefix(base, &["sealed-target "]),
        "missing-resolver" => drop_prefix(base, &["domain ", "alias ", "spell "]),
        "missing-checksum" => clear_checksums(base),
        "missing-feature" => drop_prefix(base, &["feature "]),
        "missing-config" => drop_prefix(base, &["config "]),
        "missing-toolchain" => drop_prefix(base, &["toolchain "]),
        "missing-nested" => drop_prefix(base, &["generated ", "deps "]),
        "unread-policy" => bound(base, true, false),
        "unbound-fact" => bound(base, false, true),
        "exact" => bound(base, false, false),
        _ => {
            return Err(WorldError::Qualification(format!(
                "unknown source case {case}"
            )));
        }
    };
    Ok(Some(text))
}

fn drop_prefix(text: &str, prefixes: &[&str]) -> String {
    text.lines()
        .filter(|line| !prefixes.iter().any(|prefix| line.starts_with(prefix)))
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .join("\n")
}

fn clear_checksums(text: &str) -> String {
    text.lines()
        .map(|line| {
            if !line.starts_with("member ") {
                return line.to_owned();
            }
            let mut parts = line
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if parts.len() == 9 || parts.len() == 10 {
                parts[6] = "-".to_owned();
            }
            parts.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bound(text: &str, unread: bool, unbound: bool) -> String {
    let mut body = text.to_owned();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("policy kept present\n");
    if unread {
        body.push_str("policy extra unread-token\n");
    }
    if unbound {
        body.push_str("reads fact missing|fact\n");
    } else {
        body.push_str("reads fact config|resolver\n");
    }
    body.push_str("reads policy kept\n");
    body
}

fn digest_of(lines: &[String]) -> String {
    let bytes = Sha256::digest(lines.join("\n").as_bytes());
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reason(command: &WorldCommandLib, case: &str, intended: &str) -> Result<String, WorldError> {
    let datum = command.run(&["sim".to_owned(), "source".to_owned(), case.to_owned()])?;
    let found = detail(&datum).ok_or_else(|| {
        WorldError::Qualification(format!(
            "{case} {}",
            gap_reason(&datum).unwrap_or_else(|| "unreadable".to_owned())
        ))
    })?;
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
    Ok(found)
}

fn node(detail: &str, digest: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "source-final-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/source-final-v1", "detail")
}

fn gap_reason(value: &Datum) -> Option<String> {
    text(value, "world/source-final-v1", "detail")
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

#[cfg(test)]
mod tests {
    use super::{execute, run};
    use crate::command::WorldCommandLib;

    #[test]
    fn the_captured_universe_is_final_and_the_conclusion_binds_its_reads() {
        let found = execute().expect("source");
        assert_eq!(found.complete, "complete");
        assert_eq!(found.missing_target, "missing-target");
        assert_eq!(found.missing_resolver, "missing-resolver");
        assert_eq!(found.missing_checksum, "missing-checksum");
        assert_eq!(found.missing_feature, "missing-feature");
        assert_eq!(found.missing_config, "missing-config");
        assert_eq!(found.missing_toolchain, "missing-toolchain");
        assert_eq!(found.missing_nested, "missing-nested");
        assert_eq!(found.unread_policy, "unread-policy");
        assert_eq!(found.unbound_fact, "unbound-fact");
        assert_eq!(found.exact, "exact");
        assert_ne!(found.complete, found.exact);
        assert!(!format!("{found:?}").contains("completion/claim-"));
    }

    #[test]
    fn an_unread_policy_does_not_change_the_conclusion_digest() {
        let command = WorldCommandLib::new().expect("command");
        let unread = command
            .run(&[
                "sim".to_owned(),
                "source".to_owned(),
                "unread-policy".to_owned(),
            ])
            .expect("unread");
        let exact = command
            .run(&["sim".to_owned(), "source".to_owned(), "exact".to_owned()])
            .expect("exact");
        assert_eq!(digest(&unread), digest(&exact));
        assert_ne!(digest(&exact), "");
        assert!(!digest(&exact).contains("unread-token"));
    }

    #[test]
    fn a_claim_label_is_not_a_source_case() {
        let found = run("completion/claim-10").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn digest(value: &sim_kernel::Datum) -> String {
        super::text(value, "world/source-final-v1", "digest").unwrap_or_default()
    }
}
