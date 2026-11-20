// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Required negative and mutation refusals for one binding.
//!
//! The loaded bind command classifies each case. A same-domain producer
//! presented to another receiver is `wrong-binding`. A well-formed identity
//! from another domain is `domain-confusion`. A one-byte mutation of the
//! producer digest stays in the binding domain and is `wrong-binding`.
//! The exact producer presented to itself is `accepted`. A different reason
//! is a qualification failure. This module does not derive a seal.

use sim_kernel::Datum;

use crate::WorldError;
use crate::command::WorldCommandLib;

/// Reasons the loaded bind command returned for the required negatives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefusedNegatives {
    /// Same-domain producer and receiver that do not match.
    pub wrong_binding: String,
    /// Well-formed identity whose domain is not the binding domain.
    pub domain_confusion: String,
    /// Binding-domain digest with one mutated byte.
    pub mutated: String,
    /// Exact producer presented to itself.
    pub exact: String,
}

/// Runs the required negatives through the loaded bind command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case is a gap, or a
/// case returns a reason other than the one that case exists to show.
pub fn execute() -> Result<RefusedNegatives, WorldError> {
    let command = WorldCommandLib::new()?;
    Ok(RefusedNegatives {
        wrong_binding: reason(&command, "crossed", "wrong-binding")?,
        domain_confusion: reason(&command, "confused", "domain-confusion")?,
        mutated: reason(&command, "mutated", "wrong-binding")?,
        exact: reason(&command, "exact", "accepted")?,
    })
}

fn reason(command: &WorldCommandLib, case: &str, intended: &str) -> Result<String, WorldError> {
    let datum = command.run(&["sim".to_owned(), "bind".to_owned(), case.to_owned()])?;
    let found = match detail(&datum) {
        Some(found) => found,
        None => {
            return Err(WorldError::Qualification(format!(
                "{case} {}",
                gap_reason(&datum).unwrap_or_else(|| "unreadable".to_owned())
            )));
        }
    };
    if found != intended {
        return Err(WorldError::Qualification(format!(
            "{case} returned {found}"
        )));
    }
    Ok(found)
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/bind-v1", "detail")
}

fn gap_reason(value: &Datum) -> Option<String> {
    text(value, "world/bind-gap-v1", "reason")
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
    use super::execute;

    #[test]
    fn required_negatives_fail_for_their_intended_reasons() {
        let found = execute().expect("negatives");
        assert_eq!(found.wrong_binding, "wrong-binding");
        assert_eq!(found.domain_confusion, "domain-confusion");
        assert_eq!(found.mutated, "wrong-binding");
        assert_eq!(found.exact, "accepted");
        assert_ne!(found.domain_confusion, found.wrong_binding);
        assert_ne!(found.mutated, found.domain_confusion);
    }
}
