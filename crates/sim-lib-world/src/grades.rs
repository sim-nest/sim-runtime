// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Release-grade proofs, stable keys, and a closed frontier.
//!
//! Each case asks the published catalog for one grade decision and asks
//! the published failure frontier whether any failure or continuation
//! remains. A bootstrap receipt, revoked support, a projection that
//! moves the key, an open failure set, or an open continuation each
//! return their own reason. The conclusion digest binds the rows that
//! conclusion reads. This module does not derive a completion seal.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{ProofOutcome, ProofUnit, TypedFailure, failure_frontier};

use crate::WorldError;
use crate::evidence::grade_facts;

/// Reasons the loaded grades command returns for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalGrades {
    /// Release grade, a stable key, current support, and a closed frontier.
    pub held: String,
    /// The only receipt is weaker than release grade.
    pub weak_grade: String,
    /// Support required by the release receipt is revoked.
    pub revoked: String,
    /// A moved projection is a different key.
    pub unstable_key: String,
    /// A classified failure remains and no continuation is open.
    pub frontier_open: String,
    /// A unit remains unexamined and no failure is classified.
    pub continuation_open: String,
    /// Digest of the rows the held conclusion reads.
    pub held_seal: String,
    /// Digest after the receipt is weaker than release grade.
    pub weak_grade_seal: String,
    /// Digest after support is revoked.
    pub revoked_seal: String,
    /// Digest after a projection moves the key.
    pub unstable_key_seal: String,
    /// Digest after a failure remains.
    pub frontier_open_seal: String,
    /// Digest after a continuation remains.
    pub continuation_open_seal: String,
}

/// Runs the selected grade cases through the loaded grades command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalGrades, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (held, held_seal) = sealed(&command, "held", "held")?;
    let (weak_grade, weak_grade_seal) = sealed(&command, "weak-grade", "weak-grade")?;
    let (revoked, revoked_seal) = sealed(&command, "revoked", "revoked-support")?;
    let (unstable_key, unstable_key_seal) = sealed(&command, "unstable-key", "unstable-key")?;
    let (frontier_open, frontier_open_seal) = sealed(&command, "frontier-open", "frontier-open")?;
    let (continuation_open, continuation_open_seal) =
        sealed(&command, "continuation-open", "continuation-open")?;
    let seals = [
        held_seal.as_str(),
        weak_grade_seal.as_str(),
        revoked_seal.as_str(),
        unstable_key_seal.as_str(),
        frontier_open_seal.as_str(),
        continuation_open_seal.as_str(),
    ];
    let mut unique = BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalGrades {
        held,
        weak_grade,
        revoked,
        unstable_key,
        frontier_open,
        continuation_open,
        held_seal,
        weak_grade_seal,
        revoked_seal,
        unstable_key_seal,
        frontier_open_seal,
        continuation_open_seal,
    })
}

/// Digest of the rows one existing grades case reads.
///
/// # Errors
///
/// Returns an error when the case is unknown or the digest is not a seal.
pub fn grades_digest(case: &str) -> Result<String, WorldError> {
    if !known(case) {
        return Err(WorldError::Qualification(format!(
            "unknown grades case {case}"
        )));
    }
    let class = classify(case)?;
    if !hex_seal(&class.digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok(class.digest)
}

/// Runs one named grades case.
///
/// # Errors
///
/// Returns an error when the catalog or the frontier cannot be read.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if !known(case) {
        return Ok(node("unknown-case", ""));
    }
    let class = classify(case)?;
    Ok(node(&class.detail, &class.digest))
}

/// Text the conclusion hashes for `case`.
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
        "held" | "weak-grade" | "revoked" | "unstable-key" | "frontier-open" | "continuation-open"
    )
}

struct Class {
    detail: String,
    digest: String,
    #[cfg(test)]
    report: String,
}

fn classify(case: &str) -> Result<Class, WorldError> {
    let grade = grade_facts(case)?;
    let frontier = frontier_facts(case)?;
    let frontier_text = if frontier.failures.is_empty() {
        "empty".to_owned()
    } else {
        failure_text(&frontier.failures)
    };
    let mut lines = [
        format!("decision={}", grade.decision),
        format!("grade={}", grade.grade),
        format!("key-stable={}", if grade.key_stable { "yes" } else { "no" }),
        format!("support={}", grade.support),
        format!("frontier={frontier_text}"),
        format!(
            "continuation={}",
            if frontier.open_continuation {
                "open"
            } else {
                "none"
            }
        ),
    ];
    lines.sort();
    let report = lines.join("\n");
    if report.contains("completion/claim-") {
        return Err(WorldError::Qualification("claim label".to_owned()));
    }
    let detail = detail_of(case, grade.decision, &frontier)?;
    let digest = hex(Sha256::digest(report.as_bytes()).as_slice());
    Ok(Class {
        detail: detail.to_owned(),
        digest,
        #[cfg(test)]
        report,
    })
}

fn detail_of(
    case: &str,
    decision: &str,
    frontier: &FrontierFacts,
) -> Result<&'static str, WorldError> {
    let clear = frontier.failures.is_empty() && !frontier.open_continuation;
    let matched = match case {
        "held" => decision == "reused" && clear,
        "weak-grade" => decision == "weak-grade" && clear,
        "revoked" => decision == "revoked-support" && clear,
        "unstable-key" => decision == "unstable-key" && clear,
        "frontier-open" => {
            decision == "reused" && !frontier.failures.is_empty() && !frontier.open_continuation
        }
        "continuation-open" => {
            decision == "reused" && frontier.failures.is_empty() && frontier.open_continuation
        }
        _ => false,
    };
    if !matched {
        return Err(WorldError::Qualification(format!(
            "{case} returned {decision}"
        )));
    }
    Ok(match case {
        "held" => "held",
        "weak-grade" => "weak-grade",
        "revoked" => "revoked-support",
        "unstable-key" => "unstable-key",
        "frontier-open" => "frontier-open",
        "continuation-open" => "continuation-open",
        _ => "unknown-case",
    })
}

struct FrontierFacts {
    failures: BTreeSet<TypedFailure>,
    open_continuation: bool,
}

fn frontier_facts(case: &str) -> Result<FrontierFacts, WorldError> {
    let outcomes = match case {
        "frontier-open" => outcomes(&[
            ("a", ProofOutcome::Failed),
            ("b", ProofOutcome::Passed),
            ("c", ProofOutcome::Passed),
        ]),
        "continuation-open" => {
            outcomes(&[("a", ProofOutcome::Passed), ("b", ProofOutcome::Passed)])
        }
        _ => outcomes(&[
            ("a", ProofOutcome::Passed),
            ("b", ProofOutcome::Passed),
            ("c", ProofOutcome::Passed),
        ]),
    };
    let found = failure_frontier(&units(), &outcomes, &caps(), &BTreeSet::new(), 10)
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    Ok(FrontierFacts {
        failures: found.failures,
        open_continuation: found.continuation.is_some(),
    })
}

fn units() -> Vec<ProofUnit> {
    vec![unit("a"), unit("b"), unit("c")]
}

fn unit(id: &str) -> ProofUnit {
    ProofUnit {
        id: id.to_owned(),
        depends_on: Vec::new(),
        capability: "run".to_owned(),
        invalidation_mass: 0,
        learned_cost: 0,
        critical_path_gain: 0,
        novelty_gain: 0,
        resources: Vec::new(),
    }
}

fn caps() -> BTreeSet<String> {
    BTreeSet::from(["run".to_owned()])
}

fn outcomes(pairs: &[(&str, ProofOutcome)]) -> BTreeMap<String, ProofOutcome> {
    pairs
        .iter()
        .map(|(id, outcome)| ((*id).to_owned(), *outcome))
        .collect()
}

fn failure_text(failures: &BTreeSet<TypedFailure>) -> String {
    let mut parts: Vec<_> = failures
        .iter()
        .map(|item| format!("{}:{}", item.unit, item.class))
        .collect();
    parts.sort();
    parts.join(",")
}

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "grades".to_owned(), case.to_owned()])?;
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
    let digest = text(&datum, "world/grades-v1", "digest").unwrap_or_default();
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
        tag: Symbol::qualified("world", "grades-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/grades-v1", "detail")
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
    fn release_grade_holds_and_each_fault_stands_alone() {
        let held = super::row_report("held").expect("rows");
        assert!(held.contains("decision=reused"), "{held}");
        assert!(held.contains("grade=release"), "{held}");
        assert!(held.contains("key-stable=yes"), "{held}");
        assert!(held.contains("support=current"), "{held}");
        assert!(held.contains("frontier=empty"), "{held}");
        assert!(held.contains("continuation=none"), "{held}");
        assert!(
            super::row_report("weak-grade")
                .expect("weak")
                .contains("decision=weak-grade")
        );
        assert!(
            super::row_report("weak-grade")
                .expect("weak")
                .contains("grade=bootstrap")
        );
        assert!(
            super::row_report("revoked")
                .expect("revoked")
                .contains("decision=revoked-support")
        );
        assert!(
            super::row_report("unstable-key")
                .expect("unstable")
                .contains("key-stable=no")
        );
        assert!(
            super::row_report("frontier-open")
                .expect("frontier")
                .contains("a:proof-failed")
        );
        assert!(
            super::row_report("continuation-open")
                .expect("continuation")
                .contains("continuation=open")
        );
        let found = execute().expect("grades");
        assert_eq!(found.held, "held");
        assert_eq!(found.weak_grade, "weak-grade");
        assert_eq!(found.revoked, "revoked-support");
        assert_eq!(found.unstable_key, "unstable-key");
        assert_eq!(found.frontier_open, "frontier-open");
        assert_eq!(found.continuation_open, "continuation-open");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("held", found.held_seal.as_str()),
            ("weak-grade", found.weak_grade_seal.as_str()),
            ("revoked", found.revoked_seal.as_str()),
            ("unstable-key", found.unstable_key_seal.as_str()),
            ("frontier-open", found.frontier_open_seal.as_str()),
            ("continuation-open", found.continuation_open_seal.as_str()),
        ] {
            assert_eq!(super::grades_digest(case).expect(case), seal, "{case}");
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_a_grade_case() {
        let found = run("completion/claim-11").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "grades".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/grades-v1", "digest").unwrap_or_default()
    }
}
