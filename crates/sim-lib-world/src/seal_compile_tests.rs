// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::command::{self, WorldCommandLib};

#[test]
fn the_loaded_measure_command_checks_one_admitted_input() {
    let command = WorldCommandLib::new().expect("command");
    let argv = ["sim".to_owned(), "measure".to_owned(), "seal".to_owned()];
    let first = command.run(&argv).expect("measure");
    let fields = node(&first, "world/seal-measure-v1");
    assert_eq!(text(&fields, "compile"), "checked");
    assert_eq!(text(&fields, "seal"), "same");
    assert_eq!(text(&fields, "lock"), "moved");
    assert_eq!(text(&fields, "missing"), "missing-input");
    assert_eq!(text(&fields, "corrupt"), "corrupt-input");
    assert_eq!(text(&fields, "target"), "wrong-target");
    assert_eq!(text(&fields, "stub"), "stub-only");
    assert!(bool_field(&fields, "resident"));
    assert_eq!(number(&fields, "effects"), 0);
    assert_eq!(number(&fields, "subprocesses"), 1);
    let second = command.run(&argv).expect("cached");
    assert_eq!(command::render(&first), command::render(&second));
}

fn node(value: &Datum, tag: &str) -> Vec<(Symbol, Datum)> {
    let Datum::Node { tag: found, fields } = value else {
        panic!("record {value:?}");
    };
    assert_eq!(found.as_qualified_str(), tag, "{value:?}");
    fields.clone()
}

fn text(fields: &[(Symbol, Datum)], name: &str) -> String {
    let Some(Datum::String(text)) = field(fields, name) else {
        panic!("missing {name}");
    };
    text.clone()
}

fn number(fields: &[(Symbol, Datum)], name: &str) -> u64 {
    let Some(Datum::Number(number)) = field(fields, name) else {
        panic!("missing {name}");
    };
    number
        .canonical
        .parse()
        .unwrap_or_else(|_| panic!("{name}"))
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
