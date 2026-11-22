// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Specimen refusal kept beside one later fresh admission.
//!
//! The construction specimen stays a gap. A later installed change is
//! admitted only when the fresh command returns a distinct positive record.

use std::cell::{Cell, RefCell};

use sim_kernel::{Datum, Symbol};

use crate::WorldError;

struct SuppliedChange {
    export: String,
    change: String,
}

thread_local! {
    static SUPPLIED: RefCell<Option<SuppliedChange>> = const { RefCell::new(None) };
    static HELD: Cell<u32> = const { Cell::new(0) };
}

/// Keeps the caller's fresh note while a nested fresh command runs.
///
/// The phase-union gate calls the fresh command to prove its specimen.
/// Those calls are not the change the catalog retains.
pub(crate) struct HeldNotes;

pub(crate) fn hold_notes() -> HeldNotes {
    HELD.with(|held| held.set(held.get().saturating_add(1)));
    HeldNotes
}

impl Drop for HeldNotes {
    fn drop(&mut self) {
        HELD.with(|held| held.set(held.get().saturating_sub(1)));
    }
}

/// Supplies one later change for the next catalog assembly.
///
/// The fresh command is the production writer. The catalog takes the
/// supplied export and change once. A construction specimen stays a gap.
pub(crate) fn supply(export: &str, change: &str) {
    if HELD.with(|held| held.get()) > 0 {
        return;
    }
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

    #[test]
    fn the_fresh_command_reaches_the_production_catalog() {
        let _ = take();
        let command = crate::WorldCommandLib::new().expect("command");
        let export = export_text("lantern");
        let change = "independent lantern provider/marker observed marker-next";
        let record = command
            .run(&[
                "sim".to_owned(),
                "fresh".to_owned(),
                export.clone(),
                change.to_owned(),
            ])
            .expect("fresh");
        let admitted_change = text(&record, "world/fresh-v1", "change");
        assert_eq!(admitted_change, "independent");
        assert_ne!(admitted_change, "shade-1");
        assert_eq!(text(&record, "world/fresh-v1", "seal"), "unclaimed");
        let catalog = super::super::final_catalog("").expect("catalog");
        let fresh = child(&catalog, "fresh");
        assert_eq!(
            text(&child(&fresh, "specimen"), "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        assert_eq!(
            text(&child(&fresh, "admitted"), "world/fresh-v1", "change"),
            admitted_change
        );
        assert_eq!(
            text(&child(&fresh, "admitted"), "world/fresh-v1", "project"),
            "lantern"
        );
        assert_eq!(
            text(&catalog, "world/final-catalog-v1", "seal"),
            "unclaimed"
        );
        assert!(take().is_none());

        let shade = "shade-1 lantern provider/shade observed shade-next";
        let refused = command
            .run(&[
                "sim".to_owned(),
                "fresh".to_owned(),
                export_text("lantern"),
                shade.to_owned(),
            ])
            .expect("shade");
        assert_eq!(
            text(&refused, "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        let again = super::super::final_catalog("").expect("catalog");
        assert_eq!(
            text(&child(&again, "fresh"), "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        assert_eq!(text(&again, "world/final-catalog-v1", "seal"), "unclaimed");
    }

    #[test]
    fn the_catalog_reads_the_installed_later_change() {
        let _ = take();
        let command = crate::WorldCommandLib::new().expect("command");
        let export = "\
export world/succession-export-v1
package sim-run 0.3.10 registry
package sim-lib-world 0.1.29 registry
project harbor
input provider/widget observed widget-body
input remote/hostile unavailable -
";
        let change = "later-0310 harbor provider/marker observed later-body";
        let record = command
            .run(&[
                "sim".to_owned(),
                "fresh".to_owned(),
                export.to_owned(),
                change.to_owned(),
            ])
            .expect("fresh");
        assert_eq!(text(&record, "world/fresh-v1", "change"), "later-0310");
        assert_eq!(text(&record, "world/fresh-v1", "project"), "harbor");
        assert_eq!(text(&record, "world/fresh-v1", "seal"), "unclaimed");
        assert_eq!(text(&record, "world/fresh-v1", "succession"), "portable");
        assert_eq!(text(&record, "world/fresh-v1", "linux"), "absent");
        assert_eq!(number_field(&record, "converge-second"), Some(0));
        assert_eq!(number_field(&record, "deliver-second"), Some(0));
        assert_eq!(number_field(&record, "effects"), Some(0));
        assert_eq!(number_field(&record, "subprocesses"), Some(0));
        assert_eq!(bool_field(&record, "resident"), Some(true));
        let catalog = super::super::final_catalog("").expect("catalog");
        let fresh = child(&catalog, "fresh");
        let admitted = child(&fresh, "admitted");
        assert_eq!(
            text(&child(&fresh, "specimen"), "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        assert_eq!(text(&admitted, "world/fresh-v1", "change"), "later-0310");
        assert_eq!(text(&admitted, "world/fresh-v1", "project"), "harbor");
        assert_eq!(text(&admitted, "world/fresh-v1", "seal"), "unclaimed");
        assert_eq!(text(&admitted, "world/fresh-v1", "succession"), "portable");
        assert_eq!(text(&admitted, "world/fresh-v1", "linux"), "absent");
        assert_eq!(number_field(&admitted, "converge-second"), Some(0));
        assert_eq!(number_field(&admitted, "deliver-second"), Some(0));
        assert_eq!(number_field(&admitted, "effects"), Some(0));
        assert_eq!(number_field(&admitted, "subprocesses"), Some(0));
        assert_eq!(bool_field(&admitted, "resident"), Some(true));
        assert_eq!(
            text(&catalog, "world/final-catalog-v1", "seal"),
            "unclaimed"
        );
        assert!(take().is_none());
    }

    fn bool_field(record: &Datum, name: &str) -> Option<bool> {
        let Datum::Node { fields, .. } = record else {
            return None;
        };
        fields.iter().find_map(|(field, value)| {
            (field.as_qualified_str() == name).then(|| match value {
                Datum::Bool(value) => *value,
                _ => panic!("missing {name}"),
            })
        })
    }

    fn number_field(record: &Datum, name: &str) -> Option<u64> {
        let Datum::Node { fields, .. } = record else {
            return None;
        };
        fields.iter().find_map(|(field, value)| {
            (field.as_qualified_str() == name).then(|| match value {
                Datum::Number(number) => number.canonical.parse().expect(name),
                _ => panic!("missing {name}"),
            })
        })
    }

    fn export_text(project: &str) -> String {
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

    fn child(record: &Datum, name: &str) -> Datum {
        let Datum::Node { fields, .. } = record else {
            panic!("node");
        };
        fields
            .iter()
            .find(|(field, _)| field.as_qualified_str() == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("missing {name}"))
    }

    fn text(record: &Datum, tag: &str, name: &str) -> String {
        let Datum::Node {
            tag: found, fields, ..
        } = record
        else {
            panic!("node");
        };
        assert_eq!(found.as_qualified_str(), tag);
        match fields
            .iter()
            .find(|(field, _)| field.as_qualified_str() == name)
            .map(|(_, value)| value)
        {
            Some(Datum::String(value)) => value.clone(),
            other => panic!("missing {name}: {other:?}"),
        }
    }
}
