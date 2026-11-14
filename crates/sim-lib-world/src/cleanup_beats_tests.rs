// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Datum;

use super::{
    LONG_NOW, ORIGIN_TICK, REFUSED_NOW, REFUSED_STOP, SILENCE, SPAN_NOW, SPAN_STOP, Trace,
    accept_trace, export_trace, reopen_trace, within_cleanup_gap,
};

#[test]
fn cleanup_gap_of_four_is_a_beat_and_five_is_silence() {
    assert!(within_cleanup_gap(SILENCE - 1));
    assert!(!within_cleanup_gap(SILENCE));
    let quiet = trace("span", 0, 4, "cleaning", 1, 0, vec![vec![0, 4]]);
    assert!(reopen_trace(&export_trace(&[quiet])).is_ok());
    let silent = trace("span", 0, 5, "cleaning", 1, 0, vec![vec![0, 5]]);
    assert!(matches!(
        reopen_trace(&export_trace(&[silent])),
        Err(crate::WorldError::Qualification(_))
    ));
}

#[test]
fn a_final_beat_alone_is_not_a_dense_trace() {
    let only_final = trace("span", 0, 23, "cleaning", 1, 0, vec![vec![23]]);
    assert!(matches!(
        reopen_trace(&export_trace(&[only_final])),
        Err(crate::WorldError::Qualification(_))
    ));
}

#[test]
fn measure_cleanup_uses_the_shipped_command() {
    let command = crate::WorldCommandLib::new().expect("command");
    let measured = crate::command::dispatch_argv(
        &command,
        &["sim".to_owned(), "measure".to_owned(), "cleanup".to_owned()],
    )
    .unwrap_or_else(|error| panic!("cleanup dispatch: {error}"));
    let field = |name: &str| cleanup_field(&measured, name);
    let count = |name: &str| cleanup_count(field(name));
    assert_eq!(field("command"), &Datum::String("cleanup".to_owned()));
    assert_eq!(count("refused"), 1);
    assert_eq!(count("effects"), 0);
    assert_eq!(count("subprocesses"), 0);
    assert_eq!(field("os-terminated"), &Datum::Bool(false));
    assert_eq!(field("reopened"), &Datum::Bool(true));
    assert_eq!(field("resident"), &Datum::Bool(true));
    let Datum::String(exported) = field("trace") else {
        panic!("trace is not text");
    };
    let opened = reopen_trace(exported).expect("reopen");
    assert_eq!(count("traces"), u64::try_from(opened.len()).unwrap());
    let chunks: usize = opened.iter().map(|item| item.chunks.len()).sum();
    let beats: usize = opened
        .iter()
        .flat_map(|item| &item.chunks)
        .map(Vec::len)
        .sum();
    assert_eq!(count("chunks"), u64::try_from(chunks).unwrap());
    assert_eq!(count("beats"), u64::try_from(beats).unwrap());
    assert_eq!(
        opened
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["span", "origin", "long", "refused", "closed"]
    );
    assert_cleaning(&opened[0], SPAN_STOP, SPAN_NOW);
    assert_cleaning(&opened[1], ORIGIN_TICK, ORIGIN_TICK);
    assert_eq!(opened[1].chunks, vec![vec![ORIGIN_TICK]]);
    assert_cleaning(&opened[2], SPAN_STOP, LONG_NOW);
    assert_cleaning(&opened[3], REFUSED_STOP, REFUSED_NOW);
    assert_stopped(&opened[4]);
    let mut max_gap = 0;
    for item in &opened {
        max_gap = max_gap.max(walk_gaps(item));
    }
    eprintln!(
        "cleanup beats traces {} chunks {chunks} beats {beats} max-gap {max_gap}",
        opened.len()
    );
    assert!(max_gap > 0 && within_cleanup_gap(max_gap));
    let mut corrupted = opened[0].clone();
    let beats = corrupted
        .chunks
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(beats.len() >= 3, "span did not record a dense beat list");
    let mut shifted = beats;
    shifted[1] = shifted[0] + SILENCE;
    corrupted.chunks = vec![shifted];
    assert!(matches!(
        reopen_trace(&export_trace(&[corrupted])),
        Err(crate::WorldError::Qualification(_))
    ));
}

fn assert_cleaning(trace: &Trace, origin: u64, now: u64) {
    assert_eq!(trace.origin, origin);
    assert_eq!(trace.now, now);
    assert_eq!(trace.mode, "cleaning");
    assert_eq!(trace.held, 1);
    assert_eq!(trace.closed, 0);
    assert_eq!(trace.os, 0);
    accept_trace(trace).expect("cleaning");
    let flat = trace.chunks.iter().flatten().copied().collect::<Vec<_>>();
    assert_eq!(flat.first().copied(), Some(origin));
    assert_eq!(flat.last().copied(), Some(now));
    if now.saturating_sub(origin) >= SILENCE {
        assert!(flat.len() > 1, "{}", trace.name);
    }
    for pair in flat.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(pair[1] > pair[0], "{}", trace.name);
        assert!(within_cleanup_gap(gap), "{} gap {gap}", trace.name);
    }
}

fn assert_stopped(trace: &Trace) {
    assert_eq!(trace.name, "closed");
    assert_eq!(trace.mode, "stopped");
    assert_eq!(trace.held, 0);
    assert_eq!(trace.closed, 1);
    assert_eq!(trace.os, 0);
    assert!(trace.chunks.iter().all(Vec::is_empty));
    accept_trace(trace).expect("stopped");
}

fn walk_gaps(trace: &Trace) -> u64 {
    let mut max_gap = 0;
    let mut previous = None;
    for beat in trace.chunks.iter().flatten().copied() {
        if let Some(earlier) = previous {
            max_gap = max_gap.max(beat.saturating_sub(earlier));
        }
        previous = Some(beat);
    }
    max_gap
}

fn trace(
    name: &str,
    origin: u64,
    now: u64,
    mode: &str,
    held: u64,
    closed: u64,
    chunks: Vec<Vec<u64>>,
) -> Trace {
    Trace {
        name: name.to_owned(),
        origin,
        now,
        mode: mode.to_owned(),
        held,
        closed,
        os: 0,
        chunks,
    }
}

fn cleanup_field<'a>(value: &'a Datum, name: &str) -> &'a Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("cleanup record, got {value:?}");
    };
    assert_eq!(tag.as_qualified_str(), "world/cleanup-beats-v1");
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, field)| field)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn cleanup_count(value: &Datum) -> u64 {
    let Datum::Number(number) = value else {
        panic!("count, got {value:?}");
    };
    number.canonical.parse().expect("count")
}
