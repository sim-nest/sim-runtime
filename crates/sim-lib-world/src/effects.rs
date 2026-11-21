// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selected operation settlement.
//!
//! Each case runs the operation lifecycle or the durable operation service
//! and reports the state that run produced. A verified operation whose lease
//! has expired is settled: it is not dispatched, divergent, or uncertain, and
//! a second call does not perform it again. A missing acknowledgement stays
//! dispatched. A proved absence after lease expiry is divergent. An
//! unavailable observation stays uncertain. A lease inside its window stays
//! leased. Replaying a completed or uncertain operation does not perform it
//! again. A historical unresolved dispatch is recorded as a hold, not as
//! success and not as a new attempt. This module does not derive a completion
//! seal. The operate, conduct, and schedule commands remain their own commands.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;

/// Reasons the loaded effects command returns for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalEffects {
    /// Verified, expired, and not performed again.
    pub settled: String,
    /// A missing acknowledgement is still a dispatch.
    pub dispatched: String,
    /// Observation proved a different state after the lease expired.
    pub divergent: String,
    /// Observation could not establish a value.
    pub uncertain: String,
    /// The recorded lease still contains the observation tick.
    pub leased: String,
    /// A completed or uncertain operation was not performed again.
    pub replayed: String,
    /// An unresolved historical dispatch stays a hold.
    pub historical: String,
    /// Digest of the rows the settled conclusion reads.
    pub settled_seal: String,
    /// Digest of the rows the dispatched conclusion reads.
    pub dispatched_seal: String,
    /// Digest of the rows the divergent conclusion reads.
    pub divergent_seal: String,
    /// Digest of the rows the uncertain conclusion reads.
    pub uncertain_seal: String,
    /// Digest of the rows the leased conclusion reads.
    pub leased_seal: String,
    /// Digest of the rows the replay conclusion reads.
    pub replayed_seal: String,
    /// Digest of the rows the historical conclusion reads.
    pub historical_seal: String,
}

/// Runs the selected settlement cases through the loaded effects command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalEffects, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (settled, settled_seal) = sealed(&command, "settled", "settled")?;
    let (dispatched, dispatched_seal) = sealed(&command, "dispatched", "dispatched")?;
    let (divergent, divergent_seal) = sealed(&command, "divergent", "divergent")?;
    let (uncertain, uncertain_seal) = sealed(&command, "uncertain", "uncertain")?;
    let (leased, leased_seal) = sealed(&command, "leased", "leased")?;
    let (replayed, replayed_seal) = sealed(&command, "replayed", "refused")?;
    let (historical, historical_seal) = sealed(&command, "historical", "held")?;
    let seals = [
        settled_seal.as_str(),
        dispatched_seal.as_str(),
        divergent_seal.as_str(),
        uncertain_seal.as_str(),
        leased_seal.as_str(),
        replayed_seal.as_str(),
        historical_seal.as_str(),
    ];
    let mut unique = std::collections::BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalEffects {
        settled,
        dispatched,
        divergent,
        uncertain,
        leased,
        replayed,
        historical,
        settled_seal,
        dispatched_seal,
        divergent_seal,
        uncertain_seal,
        leased_seal,
        replayed_seal,
        historical_seal,
    })
}

/// Runs one named effects case.
///
/// # Errors
///
/// Returns an error when the operation owner cannot be read.
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
/// Returns an error when the case is unknown or the operation owner refuses.
#[cfg(test)]
pub(crate) fn row_report(case: &str) -> Result<String, WorldError> {
    Ok(classify(case)?.report)
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "settled" | "dispatched" | "divergent" | "uncertain" | "leased" | "replayed" | "historical"
    )
}

fn classify(case: &str) -> Result<machine::Class, WorldError> {
    machine::classify(case)
}

#[path = "effects_machine.rs"]
mod machine;

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "effects".to_owned(), case.to_owned()])?;
    let found = detail(&datum).ok_or_else(|| fault(&format!("{case} unreadable")))?;
    if found != intended {
        return Err(fault(&format!("{case} returned {found}")));
    }
    if found.contains("completion/claim-") {
        return Err(fault(&format!("{case} carried a claim label")));
    }
    let digest = text(&datum, "world/effects-v1", "digest").unwrap_or_default();
    if !hex_seal(&digest) {
        return Err(fault(&format!("{case} seal")));
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
        tag: Symbol::qualified("world", "effects-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/effects-v1", "detail")
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

fn fault(reason: &str) -> WorldError {
    WorldError::Qualification(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{execute, run};
    use crate::command::WorldCommandLib;

    #[test]
    fn the_loaded_effects_name_settlement_and_each_fault() {
        let found = execute().expect("effects");
        assert_eq!(found.settled, "settled");
        assert_eq!(found.dispatched, "dispatched");
        assert_eq!(found.divergent, "divergent");
        assert_eq!(found.uncertain, "uncertain");
        assert_eq!(found.leased, "leased");
        assert_eq!(found.replayed, "refused");
        assert_eq!(found.historical, "held");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let settled = super::row_report("settled").expect("settled rows");
        assert!(settled.contains("lease=expired"), "{settled}");
        assert!(settled.contains("replay=refused"), "{settled}");
        let historical = super::row_report("historical").expect("historical rows");
        assert!(historical.contains("disposition=hold"), "{historical}");
        assert!(historical.contains("success=no"), "{historical}");
        assert!(historical.contains("second-attempt=absent"), "{historical}");
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("settled", found.settled_seal.as_str()),
            ("dispatched", found.dispatched_seal.as_str()),
            ("divergent", found.divergent_seal.as_str()),
            ("uncertain", found.uncertain_seal.as_str()),
            ("leased", found.leased_seal.as_str()),
            ("replayed", found.replayed_seal.as_str()),
            ("historical", found.historical_seal.as_str()),
        ] {
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_an_effects_case() {
        let found = run("completion/claim-15").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
        let operate = crate::operate::run("settled").expect("operate");
        match operate {
            sim_kernel::Datum::Node { tag, .. } => {
                assert_ne!(tag.as_qualified_str(), "world/effects-v1");
            }
            other => panic!("operate returned {other:?}"),
        }
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "effects".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/effects-v1", "digest").unwrap_or_default()
    }
}
