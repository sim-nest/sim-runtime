// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::command::WorldCommandLib;

#[test]
fn the_loaded_why_case_admits_one_stable_key() {
    let command = WorldCommandLib::new().expect("command");
    let admitted = command
        .run(&[
            "sim".to_owned(),
            "why".to_owned(),
            "evidence".to_owned(),
            "admit".to_owned(),
        ])
        .expect("admit");
    let fields = node(&admitted, "world/evidence-v1");
    assert_eq!(text(&fields, "key"), "same");
    assert_eq!(text(&fields, "receipt"), "moved");
    assert_eq!(text(&fields, "telemetry"), "same");
    assert_eq!(text(&fields, "domain"), "wrong-domain");
    assert_eq!(text(&fields, "revoked"), "revoked-support");
    assert_eq!(text(&fields, "missing"), "missing-object");
    assert_eq!(text(&fields, "storage"), "storage-id-crossing");
    assert_eq!(text(&fields, "case"), "admit");
    assert!(bool_field(&fields, "resident"));
    let unknown = command
        .run(&[
            "sim".to_owned(),
            "why".to_owned(),
            "evidence".to_owned(),
            "other".to_owned(),
        ])
        .expect("unknown");
    let gap = node(&unknown, "world/evidence-gap-v1");
    assert_eq!(text(&gap, "reason"), "unknown-case");
}

fn node(value: &Datum, tag: &str) -> Vec<(Symbol, Datum)> {
    let Datum::Node { tag: found, fields } = value else {
        panic!("record {value:?}");
    };
    assert_eq!(found.as_qualified_str(), tag);
    fields.clone()
}

fn text(fields: &[(Symbol, Datum)], name: &str) -> String {
    let Some(Datum::String(text)) = field(fields, name) else {
        panic!("missing {name}");
    };
    text.clone()
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(field(fields, name), Some(Datum::Bool(true)))
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, value)| value)
}
