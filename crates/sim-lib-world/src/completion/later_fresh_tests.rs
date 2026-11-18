// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use super::{derive, export, proved, reasons};

#[test]
fn a_later_change_is_admitted_beside_the_refused_specimen() {
    let record = super::super::fresh_set::retain(
        "construction-specimen",
        Some((
            &export("harbor"),
            "independent harbor provider/marker observed marker-next",
        )),
    )
    .expect("fresh set");
    assert_eq!(
        text(&record, "world/fresh-gap-v1", "reason"),
        "construction-specimen"
    );
    assert_eq!(
        text(&child(&record, "admitted"), "world/fresh-v1", "project"),
        "harbor"
    );
    let mut inputs = proved();
    inputs.2.fresh = record;
    assert_eq!(
        derive(&inputs).expect("later change").domain,
        super::super::SealDomain::PortableClosed
    );
    inputs.2.fresh = super::super::fresh_set::retain(
        "construction-specimen",
        Some((
            &export("lantern"),
            "shade-1 lantern provider/shade observed shade-next",
        )),
    )
    .expect("specimen");
    let gaps = derive(&inputs).expect_err("specimen");
    assert!(reasons(&gaps).contains(&"construction-specimen"));
}

fn child(record: &Datum, name: &str) -> Datum {
    let Datum::Node { tag, fields } = record else {
        panic!("fresh set");
    };
    assert_eq!(tag.as_qualified_str(), "world/fresh-set-v1");
    fields
        .iter()
        .find(|(field, _)| field.as_qualified_str() == name)
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text(record: &Datum, tag: &str, name: &str) -> String {
    let node = if let Datum::Node { tag: found, .. } = record {
        if found.as_qualified_str() == tag {
            record.clone()
        } else {
            child(record, "specimen")
        }
    } else {
        panic!("node");
    };
    let Datum::Node { fields, .. } = &node else {
        panic!("node");
    };
    match fields
        .iter()
        .find(|(field, _)| field.as_qualified_str() == name)
        .map(|(_, value)| value)
    {
        Some(Datum::String(value)) => value.clone(),
        other => panic!("missing {name}: {other:?}"),
    }
}
