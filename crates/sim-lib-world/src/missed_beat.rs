// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Missed-beat classification on the published observation path.
//!
//! Each case is its own session with one admitted worker. Ninety-nine cases
//! leave that worker blocked. One case calls the worker on another thread and
//! leaves that call inside `outcome` until the classification is encoded.
//! [`Session::observe_time`] does not call the worker. Logical time 29 stays
//! progressing. Logical time 30 is a stall, not a failure. The sample is wall
//! time from that observation through the trace line. The trace exports and
//! reopens. A modeled stall does not claim process death.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    Admit, Liveness, LogicalTime, ModelWorker, ProofUnit, ScheduleError, Session, WorkerView,
};

use crate::WorldError;
use crate::query::{self, HostWork};

pub(crate) const CASES: usize = 100;
const HUNG: usize = 0;
/// Logical seconds. A beat at 29 stays fresh. A beat at 30 is missed.
pub(crate) const BOUND: u64 = 30;
const EARLY: u64 = BOUND - 1;
/// Fixed classification-dispatch bound: thirty seconds, in microseconds.
pub(crate) const CEILING_US: u64 = 30_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    index: usize,
    kind: &'static str,
    early_at: u64,
    early: &'static str,
    stall_at: u64,
    class: &'static str,
    since: u64,
    queue: u64,
    failures: u64,
    os: u64,
}

/// Reports whether a nearest-rank p95 in microseconds meets the dispatch bound.
///
/// `p95-ms` stays the whole-millisecond truncation of that sample. This
/// predicate admits 30_000_000 microseconds and refuses 30_000_001.
#[must_use]
pub(crate) fn within_missed_ceiling(p95_us: u64) -> bool {
    p95_us <= CEILING_US
}

pub(crate) struct MeasuredMissed {
    pub(crate) record: Datum,
    pub(crate) within_ceiling: bool,
}

/// Classifies one missed beat for [`CASES`] admitted workers.
///
/// # Errors
/// Returns an error when a worker is not admitted, classification is not an
/// independent stall, or the exported trace does not reopen.
pub(crate) fn measure_missed() -> Result<MeasuredMissed, WorldError> {
    let mut rows = Vec::with_capacity(CASES);
    let mut raw_us = Vec::with_capacity(CASES);
    for index in 0..CASES {
        let (row, elapsed) = classify_case(index)?;
        rows.push(row);
        raw_us.push(elapsed);
    }
    let hung = rows.iter().filter(|row| row.kind == "hung").count();
    if hung != 1 || rows[HUNG].kind != "hung" {
        return Err(WorldError::Qualification(
            "missed-beat measurement omitted the worker that does not return".to_owned(),
        ));
    }
    let exported = export_trace(&rows);
    let opened = reopen_trace(&exported)?;
    if opened != rows {
        return Err(WorldError::Qualification(
            "reopened missed-beat trace disagreed".to_owned(),
        ));
    }
    let corrupt = corrupt_trace(&exported);
    if reopen_trace(&corrupt).ok().as_ref() == Some(&rows) {
        return Err(WorldError::Qualification(
            "changed missed-beat trace matched the export".to_owned(),
        ));
    }
    let mut ordered = raw_us.clone();
    ordered.sort_unstable();
    let p95_us = ordered[query::nearest_rank_p95(CASES)];
    let work = HostWork::default();
    let receipt = Receipt {
        trace: exported,
        raw_us,
        hung,
        p95_ms: p95_us / 1_000,
        effects: work.effects(),
        subprocesses: work.subprocesses(),
        os_terminated: rows.iter().any(|row| row.os != 0),
        reopened: opened == rows,
    };
    Ok(MeasuredMissed {
        record: record(&receipt),
        within_ceiling: within_missed_ceiling(p95_us),
    })
}

fn classify_case(index: usize) -> Result<(Row, u64), WorldError> {
    if index == HUNG {
        classify_hung(index)
    } else {
        let mut session = open_session(&unit_id(index))?;
        classify_session(&mut session, index, "blocked")
    }
}

fn classify_hung(index: usize) -> Result<(Row, u64), WorldError> {
    let id = unit_id(index);
    let mut session = open_session(&id)?;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = Hold {
        entered: entered_tx,
        release: release_rx,
    };
    let handle = thread::spawn(move || worker.outcome(&id));
    if entered_rx.recv().is_err() {
        let _ = release_tx.send(());
        let _ = handle.join();
        return Err(WorldError::Qualification(
            "hung worker did not enter".to_owned(),
        ));
    }
    let result = classify_session(&mut session, index, "hung");
    let _ = release_tx.send(());
    let joined = handle.join();
    let (row, elapsed) = result?;
    if joined.is_err() {
        return Err(WorldError::Qualification("hung worker panicked".to_owned()));
    }
    Ok((row, elapsed))
}

struct Hold {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl ModelWorker for Hold {
    fn outcome(&self, _id: &str) -> WorkerView {
        let _ = self.entered.send(());
        let _ = self.release.recv();
        WorkerView::Blocked
    }
}

fn classify_session(
    session: &mut Session,
    index: usize,
    kind: &'static str,
) -> Result<(Row, u64), WorldError> {
    let id = unit_id(index);
    if session.work_len() != 1 || session.os_terminated() {
        return Err(WorldError::Qualification(
            "missed-beat saw a queue that was not one admitted worker".to_owned(),
        ));
    }
    let early = session.observe_time(LogicalTime(EARLY)).map_err(fault)?;
    let (early_at, early_name) = progressing(&early)?;
    let started = Instant::now();
    let live = session.observe_time(LogicalTime(BOUND)).map_err(fault)?;
    let (stall_at, class, since) = stalled(&live, &id)?;
    let queue = count_usize(session.work_len());
    let failures = failure_count(session)?;
    let os = u64::from(session.os_terminated());
    let row = Row {
        index,
        kind,
        early_at,
        early: early_name,
        stall_at,
        class,
        since,
        queue,
        failures,
        os,
    };
    let encoded = std::hint::black_box(trace_line(&row));
    let elapsed = micros(started.elapsed());
    let admitted = session.admit_next(LogicalTime(BOUND)).map_err(fault)?;
    if encoded.is_empty()
        || early_at != EARLY
        || early_name != "progressing"
        || stall_at != BOUND
        || class != "stalled"
        || since != 0
        || queue != 1
        || failures != 0
        || os != 0
        || admitted != Admit::Idle
    {
        return Err(WorldError::Qualification(
            "missed-beat classification did not leave the blocked worker".to_owned(),
        ));
    }
    Ok((row, elapsed))
}

fn progressing(live: &Liveness) -> Result<(u64, &'static str), WorldError> {
    match live {
        Liveness::Progressing { beat } if beat.0 == EARLY => Ok((beat.0, "progressing")),
        _ => Err(WorldError::Qualification(
            "missed-beat classification stalled before 30 logical seconds".to_owned(),
        )),
    }
}

fn stalled(live: &Liveness, id: &str) -> Result<(u64, &'static str, u64), WorldError> {
    match live {
        Liveness::Stalled { unit, since } if unit == id => Ok((BOUND, "stalled", since.0)),
        _ => Err(WorldError::Qualification(
            "missed-beat classification was not a stall".to_owned(),
        )),
    }
}

fn failure_count(session: &Session) -> Result<u64, WorldError> {
    let frontier = session.failure_set(10).map_err(fault)?;
    Ok(count_usize(frontier.failures.len()))
}

fn open_session(id: &str) -> Result<Session, WorldError> {
    let mut session = Session::new(
        vec![unit(id)],
        BTreeSet::from(["run".to_owned()]),
        BTreeMap::new(),
    )
    .map_err(fault)?;
    match session.admit_next(LogicalTime(0)).map_err(fault)? {
        Admit::Admitted { unit } if unit == id => Ok(session),
        _ => Err(WorldError::Qualification(
            "worker was not admitted".to_owned(),
        )),
    }
}

fn unit(id: &str) -> ProofUnit {
    ProofUnit {
        id: id.to_owned(),
        depends_on: Vec::new(),
        capability: "run".to_owned(),
        invalidation_mass: 1,
        learned_cost: 1,
        critical_path_gain: 0,
        novelty_gain: 0,
        resources: Vec::new(),
    }
}

fn unit_id(index: usize) -> String {
    format!("w{index:03}")
}

fn export_trace(rows: &[Row]) -> String {
    let mut text = String::new();
    for row in rows {
        text.push_str(&trace_line(row));
        text.push('\n');
    }
    text
}

fn trace_line(row: &Row) -> String {
    format!(
        "{:03} {} missed {} {} {} {} {} {} {} {}",
        row.index,
        row.kind,
        row.early_at,
        row.early,
        row.stall_at,
        row.class,
        row.since,
        row.queue,
        row.failures,
        row.os
    )
}

fn reopen_trace(exported: &str) -> Result<Vec<Row>, WorldError> {
    if exported.is_empty() || !exported.ends_with('\n') {
        return Err(WorldError::Qualification(
            "missed-beat trace export is empty".to_owned(),
        ));
    }
    let mut rows = Vec::new();
    for line in exported.lines() {
        rows.push(parse_line(line)?);
    }
    if rows.len() != CASES {
        return Err(WorldError::Qualification(
            "reopened missed-beat trace has the wrong case count".to_owned(),
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.index != index {
            return Err(WorldError::Qualification(
                "reopened missed-beat trace skipped a case".to_owned(),
            ));
        }
    }
    let hung = rows.iter().filter(|row| row.kind == "hung").count();
    if hung != 1 || rows[HUNG].kind != "hung" {
        return Err(WorldError::Qualification(
            "reopened missed-beat trace lost the worker that does not return".to_owned(),
        ));
    }
    Ok(rows)
}

fn parse_line(line: &str) -> Result<Row, WorldError> {
    let mut parts = line.split(' ');
    let index = parse_usize(parts.next())?;
    let kind = kind_name(parts.next())?;
    let verb = parts.next();
    let early_at = parse_u64(parts.next())?;
    let early = class_name(parts.next())?;
    let stall_at = parse_u64(parts.next())?;
    let class = class_name(parts.next())?;
    let since = parse_u64(parts.next())?;
    let queue = parse_u64(parts.next())?;
    let failures = parse_u64(parts.next())?;
    let os = parse_u64(parts.next())?;
    if parts.next().is_some() || verb != Some("missed") {
        return Err(WorldError::Qualification(
            "missed-beat trace line is not a classification".to_owned(),
        ));
    }
    if (index == HUNG) != (kind == "hung")
        || early_at != EARLY
        || early != "progressing"
        || stall_at != BOUND
        || class != "stalled"
        || since != 0
        || queue != 1
        || failures != 0
        || os != 0
    {
        return Err(WorldError::Qualification(
            "missed-beat trace line is outside the stall contract".to_owned(),
        ));
    }
    Ok(Row {
        index,
        kind,
        early_at,
        early,
        stall_at,
        class,
        since,
        queue,
        failures,
        os,
    })
}

fn kind_name(token: Option<&str>) -> Result<&'static str, WorldError> {
    match token {
        Some("blocked") => Ok("blocked"),
        Some("hung") => Ok("hung"),
        _ => Err(WorldError::Qualification(
            "missed-beat trace names an unknown worker".to_owned(),
        )),
    }
}

fn class_name(token: Option<&str>) -> Result<&'static str, WorldError> {
    match token {
        Some("progressing") => Ok("progressing"),
        Some("stalled") => Ok("stalled"),
        _ => Err(WorldError::Qualification(
            "missed-beat trace names an unknown class".to_owned(),
        )),
    }
}

fn corrupt_trace(exported: &str) -> String {
    let mut bytes = exported.as_bytes().to_vec();
    if let Some(pos) = bytes.iter().rposition(|byte| *byte == b'0') {
        bytes[pos] = b'1';
    }
    String::from_utf8(bytes).unwrap_or_default()
}

struct Receipt {
    trace: String,
    raw_us: Vec<u64>,
    hung: usize,
    p95_ms: u64,
    effects: u64,
    subprocesses: u64,
    os_terminated: bool,
    reopened: bool,
}

fn record(receipt: &Receipt) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "missed-beat-v1"),
        fields: vec![
            (Symbol::new("command"), Datum::String("missed".to_owned())),
            (Symbol::new("cases"), query::count_datum(count_usize(CASES))),
            (
                Symbol::new("hung"),
                query::count_datum(count_usize(receipt.hung)),
            ),
            (
                Symbol::new("samples"),
                query::count_datum(count_usize(receipt.raw_us.len())),
            ),
            (Symbol::new("p95-ms"), query::count_datum(receipt.p95_ms)),
            (
                Symbol::new("raw-us"),
                Datum::Vector(
                    receipt
                        .raw_us
                        .iter()
                        .copied()
                        .map(query::count_datum)
                        .collect(),
                ),
            ),
            (Symbol::new("effects"), query::count_datum(receipt.effects)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(receipt.subprocesses),
            ),
            (
                Symbol::new("os-terminated"),
                Datum::Bool(receipt.os_terminated),
            ),
            (Symbol::new("reopened"), Datum::Bool(receipt.reopened)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("trace"), Datum::String(receipt.trace.clone())),
        ],
    }
}

fn parse_usize(token: Option<&str>) -> Result<usize, WorldError> {
    let value = parse_u64(token)?;
    usize::try_from(value)
        .map_err(|_| WorldError::Qualification("missed-beat trace index does not fit".to_owned()))
}

fn parse_u64(token: Option<&str>) -> Result<u64, WorldError> {
    token
        .ok_or_else(|| WorldError::Qualification("missed-beat trace line is short".to_owned()))?
        .parse()
        .map_err(|_| {
            WorldError::Qualification("missed-beat trace number is not decimal".to_owned())
        })
}

fn count_usize(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn micros(elapsed: std::time::Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

fn fault(error: ScheduleError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

#[cfg(test)]
#[path = "missed_beat_tests.rs"]
mod tests;
