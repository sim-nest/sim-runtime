// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use super::{
    BOUND, CASES, CEILING_US, EARLY, corrupt_trace, parse_line, reopen_trace, within_missed_ceiling,
};

#[test]
fn missed_ceiling_compares_raw_microseconds() {
    assert!(within_missed_ceiling(CEILING_US));
    assert!(!within_missed_ceiling(CEILING_US + 1));
    assert_eq!((CEILING_US + 1) / 1_000, CEILING_US / 1_000);
}

#[test]
fn early_stall_is_outside_the_contract() {
    let good = "000 hung missed 29 progressing 30 stalled 0 1 0 0";
    let row = parse_line(good).expect("contract line");
    assert_eq!(row.early_at, EARLY);
    assert_eq!(row.stall_at, BOUND);
    assert_eq!(row.early, "progressing");
    assert_eq!(row.class, "stalled");
    assert_eq!(row.since, 0);
    assert_eq!(row.os, 0);
    assert!(parse_line("000 hung missed 29 stalled 30 stalled 0 1 0 0").is_err());
    assert!(parse_line("000 hung missed 30 progressing 30 stalled 0 1 0 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 29 stalled 0 1 0 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 30 progressing 0 1 0 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 30 stalled 1 1 0 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 30 stalled 0 0 0 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 30 stalled 0 1 1 0").is_err());
    assert!(parse_line("000 hung missed 29 progressing 30 stalled 0 1 0 1").is_err());
    assert!(parse_line("001 hung missed 29 progressing 30 stalled 0 1 0 0").is_err());
    assert!(parse_line("000 blocked missed 29 progressing 30 stalled 0 1 0 0").is_err());
}

#[test]
fn measure_missed_uses_the_shipped_command() {
    let command = crate::WorldCommandLib::new().expect("command");
    let measured = crate::command::dispatch_argv(
        &command,
        &["sim".to_owned(), "measure".to_owned(), "missed".to_owned()],
    )
    .unwrap_or_else(|error| panic!("missed dispatch: {error}"));
    let field = |name: &str| missed_field(&measured, name);
    let count = |name: &str| missed_count(field(name));
    assert_eq!(field("command"), &Datum::String("missed".to_owned()));
    assert_eq!(count("cases"), u64::try_from(CASES).unwrap());
    assert_eq!(count("hung"), 1);
    assert_eq!(count("samples"), u64::try_from(CASES).unwrap());
    assert_eq!(count("effects"), 0);
    assert_eq!(count("subprocesses"), 0);
    assert_eq!(field("os-terminated"), &Datum::Bool(false));
    assert_eq!(field("reopened"), &Datum::Bool(true));
    assert_eq!(field("resident"), &Datum::Bool(true));
    let Datum::Vector(raw) = field("raw-us") else {
        panic!("raw-us is not a vector");
    };
    assert_eq!(raw.len(), CASES);
    let mut ordered: Vec<u64> = raw.iter().map(missed_count).collect();
    let rank = (ordered.len() * 95).div_ceil(100) - 1;
    ordered.sort_unstable();
    let p95_us = ordered[rank];
    assert_eq!(count("p95-ms"), p95_us / 1_000);
    let Datum::String(trace) = field("trace") else {
        panic!("trace is not text");
    };
    let opened = reopen_trace(trace).expect("reopen");
    assert_eq!(opened.len(), CASES);
    assert_eq!(opened[0].kind, "hung");
    assert!(opened[1..].iter().all(|row| row.kind == "blocked"));
    assert!(opened.iter().all(|row| {
        row.early_at == EARLY
            && row.early == "progressing"
            && row.stall_at == BOUND
            && row.class == "stalled"
            && row.since == 0
            && row.queue == 1
            && row.failures == 0
            && row.os == 0
    }));
    eprintln!(
        "missed-beat traces {} hung 1 early {} class stalled since 0 queue 1 failures 0 os 0 p95 {} ms raw-us {p95_us}",
        opened.len(),
        opened[0].early_at,
        p95_us / 1_000
    );
    assert!(within_missed_ceiling(p95_us));
    let corrupt = corrupt_trace(trace);
    assert!(reopen_trace(&corrupt).ok().as_ref() != Some(&opened));
}

fn missed_field<'a>(value: &'a Datum, name: &str) -> &'a Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("missed record, got {value:?}");
    };
    assert_eq!(tag.as_qualified_str(), "world/missed-beat-v1");
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, field)| field)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn missed_count(value: &Datum) -> u64 {
    let Datum::Number(number) = value else {
        panic!("count, got {value:?}");
    };
    number.canonical.parse().expect("count")
}
