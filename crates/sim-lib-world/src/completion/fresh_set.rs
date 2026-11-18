// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Specimen refusal kept beside one later fresh admission.
//!
//! The construction specimen stays a gap. A later installed change is
//! admitted only when the fresh command returns a distinct positive record.

use std::cell::RefCell;

use sim_kernel::{Datum, Symbol};

use crate::WorldError;

struct SuppliedChange {
    export: String,
    change: String,
}

thread_local! {
    static SUPPLIED: RefCell<Option<SuppliedChange>> = const { RefCell::new(None) };
}

/// Supplies one later change for the next catalog assembly.
#[cfg(test)]
pub(crate) fn supply(export: &str, change: &str) {
    SUPPLIED.with(|slot| {
        *slot.borrow_mut() = Some(SuppliedChange {
            export: export.to_owned(),
            change: change.to_owned(),
        });
    });
}

pub(crate) fn take() -> Option<(String, String)> {
    SUPPLIED.with(|slot| {
        slot.borrow_mut()
            .take()
            .map(|supplied| (supplied.export, supplied.change))
    })
}

/// Retains the specimen gap, and a positive fresh record when one was supplied.
///
/// # Errors
///
/// Returns an error when the fresh command cannot read the supplied change.
pub(crate) fn retain(reason: &str, supplied: Option<(&str, &str)>) -> Result<Datum, WorldError> {
    let specimen = crate::fresh::gap_record(reason);
    let Some((export, change)) = supplied else {
        return Ok(specimen);
    };
    let later = crate::fresh::run(export, change)?;
    if positive(&later) {
        Ok(set(specimen, later))
    } else {
        Ok(specimen)
    }
}

fn positive(record: &Datum) -> bool {
    let Datum::Node { tag, fields } = record else {
        return false;
    };
    tag.as_qualified_str() == "world/fresh-v1"
        && fields.iter().any(|(name, value)| {
            name.as_qualified_str() == "change"
                && matches!(value, Datum::String(text) if text != "shade-1" && !text.is_empty())
        })
}

fn set(specimen: Datum, admitted: Datum) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "fresh-set-v1"),
        fields: vec![
            (Symbol::new("admitted"), admitted),
            (Symbol::new("specimen"), specimen),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supplied_change_is_taken_once() {
        supply("export world/succession-export-v1\n", "independent");
        let (export, change) = take().expect("supplied");
        assert_eq!(export, "export world/succession-export-v1\n");
        assert_eq!(change, "independent");
        assert!(take().is_none());
    }
}
