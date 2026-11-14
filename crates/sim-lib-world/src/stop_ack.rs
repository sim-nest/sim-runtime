// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stop acknowledgement on the published control path.
//!
//! Each case is its own session with one admitted worker. Ninety-nine cases
//! deliver a blocked view and stop. One case calls the worker on another
//! thread and leaves that call inside `outcome` until the acknowledgement is
//! encoded. [`Session::submit`] does not call the worker. The sample is wall
//! time from that submit through the trace line. Logical time is only the
//! control entry's tick. The trace exports and reopens. A changed byte does
//! not match. Closing the modeled queue does not claim process death.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    Admit, ControlAck, ControlIntent, LogicalTime, Mode, ModelWorker, ProofUnit, ScheduleError,
    Session, WorkerView,
};

use crate::WorldError;
use crate::query::{self, HostWork};

pub(crate) const CASES: usize = 100;
const HUNG: usize = 0;
/// Fixed stop-acknowledgement bound: one second, in microseconds.
pub(crate) const CEILING_US: u64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    index: usize,
    kind: &'static str,
    at: u64,
    mode: &'static str,
    queue: u64,
    os: u64,
}

/// Reports whether a nearest-rank p95 in microseconds meets the stop bound.
///
/// `p95-ms` stays the whole-millisecond truncation of that sample. This
/// predicate admits 1_000_000 microseconds and refuses 1_000_001.
#[must_use]
pub(crate) fn within_stop_ceiling(p95_us: u64) -> bool {
    p95_us <= CEILING_US
}

pub(crate) struct MeasuredStop {
    pub(crate) record: Datum,
    pub(crate) within_ceiling: bool,
}

/// Measures stop acknowledgement for [`CASES`] blocked workers.
///
/// # Errors
/// Returns an error when a worker is not admitted, the acknowledgement is
/// not an independent stop, or the exported trace does not reopen.
pub(crate) fn measure_stop() -> Result<MeasuredStop, WorldError> {
    let mut rows = Vec::with_capacity(CASES);
    let mut raw_us = Vec::with_capacity(CASES);
    for index in 0..CASES {
        let (row, elapsed) = if index == HUNG {
            ack_hung(index)?
        } else {
            ack_blocked(index)?
        };
        rows.push(row);
        raw_us.push(elapsed);
    }
    let hung = rows.iter().filter(|row| row.kind == "hung").count();
    if hung != 1 || rows[HUNG].kind != "hung" {
        return Err(WorldError::Qualification(
            "stop measurement omitted the worker that does not return".to_owned(),
        ));
    }
    let exported = export_trace(&rows);
    let opened = reopen_trace(&exported)?;
    if opened != rows {
        return Err(WorldError::Qualification(
            "reopened stop trace disagreed".to_owned(),
        ));
    }
    let corrupt = corrupt_trace(&exported);
    if reopen_trace(&corrupt).ok().as_ref() == Some(&rows) {
        return Err(WorldError::Qualification(
            "changed stop trace matched the export".to_owned(),
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
    Ok(MeasuredStop {
        record: record(&receipt),
        within_ceiling: within_stop_ceiling(p95_us),
    })
}

fn ack_blocked(index: usize) -> Result<(Row, u64), WorldError> {
    let id = unit_id(index);
    let mut session = open_session(&id)?;
    session
        .deliver_outcome(&id, WorkerView::Blocked)
        .map_err(fault)?;
    ack_session(&mut session, index, "blocked")
}

fn ack_hung(index: usize) -> Result<(Row, u64), WorldError> {
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
    let result = ack_session(&mut session, index, "hung");
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

fn ack_session(
    session: &mut Session,
    index: usize,
    kind: &'static str,
) -> Result<(Row, u64), WorldError> {
    if session.work_len() != 1 || !session.control_log().is_empty() || session.os_terminated() {
        return Err(WorldError::Qualification(
            "stop saw a queue that was not one blocked worker".to_owned(),
        ));
    }
    let started = Instant::now();
    let ack = session
        .submit(ControlIntent::Stop, LogicalTime(1))
        .map_err(fault)?;
    let row = row_from(session, index, kind, &ack);
    let encoded = std::hint::black_box(trace_line(&row));
    let elapsed = micros(started.elapsed());
    if encoded.is_empty()
        || ack.intent != ControlIntent::Stop
        || row.mode != "stopped"
        || row.queue != 0
        || row.os != 0
        || session.control_log().len() != 1
        || session.control_log()[0].intent != ControlIntent::Stop
    {
        return Err(WorldError::Qualification(
            "stop acknowledgement did not leave the blocked worker".to_owned(),
        ));
    }
    Ok((row, elapsed))
}

fn row_from(session: &Session, index: usize, kind: &'static str, ack: &ControlAck) -> Row {
    Row {
        index,
        kind,
        at: ack.at.0,
        mode: mode_name(ack.mode),
        queue: count_usize(session.work_len()),
        os: u64::from(session.os_terminated()),
    }
}

fn mode_name(mode: Mode) -> &'static str {
    if mode == Mode::Stopped {
        "stopped"
    } else {
        "other"
    }
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
        "{:03} {} stop {} {} {} {}",
        row.index, row.kind, row.at, row.mode, row.queue, row.os
    )
}

fn reopen_trace(exported: &str) -> Result<Vec<Row>, WorldError> {
    if exported.is_empty() || !exported.ends_with('\n') {
        return Err(WorldError::Qualification(
            "stop trace export is empty".to_owned(),
        ));
    }
    let mut rows = Vec::new();
    for line in exported.lines() {
        rows.push(parse_line(line)?);
    }
    if rows.len() != CASES {
        return Err(WorldError::Qualification(
            "reopened stop trace has the wrong case count".to_owned(),
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.index != index {
            return Err(WorldError::Qualification(
                "reopened stop trace skipped a case".to_owned(),
            ));
        }
    }
    Ok(rows)
}

fn parse_line(line: &str) -> Result<Row, WorldError> {
    let mut parts = line.split(' ');
    let index = parse_usize(parts.next())?;
    let kind = kind_name(parts.next())?;
    let verb = parts.next();
    let at = parse_u64(parts.next())?;
    let mode = mode_token(parts.next())?;
    let queue = parse_u64(parts.next())?;
    let os = parse_u64(parts.next())?;
    if parts.next().is_some() || verb != Some("stop") {
        return Err(WorldError::Qualification(
            "stop trace line is not an acknowledgement".to_owned(),
        ));
    }
    Ok(Row {
        index,
        kind,
        at,
        mode,
        queue,
        os,
    })
}

fn kind_name(token: Option<&str>) -> Result<&'static str, WorldError> {
    match token {
        Some("blocked") => Ok("blocked"),
        Some("hung") => Ok("hung"),
        _ => Err(WorldError::Qualification(
            "stop trace names an unknown worker".to_owned(),
        )),
    }
}

fn mode_token(token: Option<&str>) -> Result<&'static str, WorldError> {
    match token {
        Some("stopped") => Ok("stopped"),
        Some("other") => Ok("other"),
        _ => Err(WorldError::Qualification(
            "stop trace names an unknown mode".to_owned(),
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
        tag: Symbol::qualified("world", "stop-ack-v1"),
        fields: vec![
            (Symbol::new("command"), Datum::String("stop".to_owned())),
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
        .map_err(|_| WorldError::Qualification("stop trace index does not fit".to_owned()))
}

fn parse_u64(token: Option<&str>) -> Result<u64, WorldError> {
    token
        .ok_or_else(|| WorldError::Qualification("stop trace line is short".to_owned()))?
        .parse()
        .map_err(|_| WorldError::Qualification("stop trace number is not decimal".to_owned()))
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
mod tests {
    use sim_kernel::Datum;

    use super::{CASES, CEILING_US, corrupt_trace, reopen_trace, within_stop_ceiling};

    #[test]
    fn stop_ceiling_compares_raw_microseconds() {
        assert!(within_stop_ceiling(CEILING_US));
        assert!(!within_stop_ceiling(CEILING_US + 1));
        assert_eq!((CEILING_US + 1) / 1_000, 1_000);
    }

    #[test]
    fn measure_stop_uses_the_shipped_command() {
        let command = crate::WorldCommandLib::new().expect("command");
        let measured = crate::command::dispatch_argv(
            &command,
            &["sim".to_owned(), "measure".to_owned(), "stop".to_owned()],
        )
        .unwrap_or_else(|error| panic!("stop dispatch: {error}"));
        let field = |name: &str| stop_field(&measured, name);
        let count = |name: &str| stop_count(field(name));
        assert_eq!(field("command"), &Datum::String("stop".to_owned()));
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
        let mut ordered: Vec<u64> = raw.iter().map(stop_count).collect();
        let rank = (ordered.len() * 95).div_ceil(100) - 1;
        ordered.sort_unstable();
        let p95_us = ordered[rank];
        assert_eq!(count("p95-ms"), p95_us / 1_000);
        eprintln!(
            "stop acknowledgement p95 {} ms raw-us {p95_us}",
            p95_us / 1_000
        );
        assert!(within_stop_ceiling(p95_us));
        let Datum::String(trace) = field("trace") else {
            panic!("trace is not text");
        };
        let opened = reopen_trace(trace).expect("reopen");
        assert_eq!(opened.len(), CASES);
        assert_eq!(opened[0].kind, "hung");
        assert!(opened[1..].iter().all(|row| row.kind == "blocked"));
        assert!(
            opened
                .iter()
                .all(|row| row.os == 0 && row.queue == 0 && row.mode == "stopped")
        );
        let corrupt = corrupt_trace(trace);
        assert!(reopen_trace(&corrupt).ok().as_ref() != Some(&opened));
    }

    fn stop_field<'a>(value: &'a Datum, name: &str) -> &'a Datum {
        let Datum::Node { tag, fields } = value else {
            panic!("stop record, got {value:?}");
        };
        assert_eq!(tag.as_qualified_str(), "world/stop-ack-v1");
        fields
            .iter()
            .find(|(symbol, _)| symbol.as_qualified_str() == name)
            .map(|(_, field)| field)
            .unwrap_or_else(|| panic!("missing {name}"))
    }

    fn stop_count(value: &Datum) -> u64 {
        let Datum::Number(number) = value else {
            panic!("count, got {value:?}");
        };
        number.canonical.parse().expect("count")
    }
}
