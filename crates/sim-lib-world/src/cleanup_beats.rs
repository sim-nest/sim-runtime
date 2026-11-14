// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleanup beats on every portable trace that owns a modeled resource.
//!
//! Each trace stops through [`Session::submit`] and then calls
//! [`Session::observe_time`] until the last beat equals the requested tick.
//! A gap of five logical ticks is silence. Closing the modeled lease does
//! not claim process death.

use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    Admit, ControlIntent, Liveness, LogicalTime, Mode, ProofUnit, ResourceAsk, ScheduleError,
    Session,
};

use crate::WorldError;
use crate::query::{self, HostWork};

/// Logical gap that is still a beat. Five ticks is silence.
pub(crate) const SILENCE: u64 = 5;
const OBSERVE_PASSES: usize = 8;
const HOLDER: &str = "holder";
const SPAN_STOP: u64 = 0;
const SPAN_NOW: u64 = 23;
const ORIGIN_TICK: u64 = 20_000;
const LONG_NOW: u64 = 20_000;
const REFUSED_STOP: u64 = 1;
const REFUSED_DRAIN: u64 = 9;
const REFUSED_NOW: u64 = 3;

/// Reports whether a logical gap stays inside the cleanup beat.
///
/// A gap of four ticks is a beat. A gap of five ticks is silence.
#[must_use]
pub(crate) fn within_cleanup_gap(gap: u64) -> bool {
    gap < SILENCE
}

/// Measures cleanup beats for every modeled trace in this command.
///
/// # Errors
/// Returns an error when a lease is missing, a span is silent, a refused
/// drain moves logical time, or the exported trace does not reopen.
pub(crate) fn measure_cleanup() -> Result<Datum, WorldError> {
    let mut span = open_holder()?;
    let mut traces = Vec::new();
    traces.push(cleaning(&mut span, "span", SPAN_STOP, SPAN_NOW)?);
    let mut origin = open_holder()?;
    traces.push(cleaning(&mut origin, "origin", ORIGIN_TICK, ORIGIN_TICK)?);
    let mut long = open_holder()?;
    traces.push(cleaning(&mut long, "long", SPAN_STOP, LONG_NOW)?);
    let mut refused_session = open_holder()?;
    let refused = refuse_drain(&mut refused_session)?;
    traces.push(cleaning_after_stop(
        &mut refused_session,
        "refused",
        REFUSED_STOP,
        REFUSED_NOW,
    )?);
    traces.push(close_holder(&mut span, SPAN_STOP, SPAN_NOW)?);
    for trace in &traces {
        accept_trace(trace)?;
    }
    let exported = export_trace(&traces);
    let opened = reopen_trace(&exported)?;
    if opened != traces {
        return Err(silence("reopened cleanup trace disagreed"));
    }
    expect_names(&opened)?;
    let work = HostWork::default();
    Ok(record(
        &opened,
        &exported,
        refused,
        work.effects(),
        work.subprocesses(),
    ))
}

fn cleaning(session: &mut Session, name: &str, stop: u64, now: u64) -> Result<Trace, WorldError> {
    stop_at(session, stop)?;
    cleaning_after_stop(session, name, stop, now)
}

fn cleaning_after_stop(
    session: &mut Session,
    name: &str,
    origin: u64,
    now: u64,
) -> Result<Trace, WorldError> {
    let (held, os) = cleaning_state(session)?;
    let chunks = observe_until(session, now)?;
    require_span(&chunks, origin, now)?;
    let (held_after, os_after) = cleaning_state(session)?;
    if held_after != held || os_after != os {
        return Err(silence("cleanup observation dropped the modeled lease"));
    }
    Ok(Trace {
        name: name.to_owned(),
        origin,
        now,
        mode: "cleaning".to_owned(),
        held,
        closed: 0,
        os,
        chunks,
    })
}

fn refuse_drain(session: &mut Session) -> Result<u64, WorldError> {
    stop_at(session, REFUSED_STOP)?;
    match session.submit(ControlIntent::Drain, LogicalTime(REFUSED_DRAIN)) {
        Err(ScheduleError::Refused("stopped")) => Ok(1),
        Ok(_) => Err(silence("drain after stop was accepted")),
        Err(error) => Err(fault(error)),
    }
}

fn close_holder(session: &mut Session, origin: u64, now: u64) -> Result<Trace, WorldError> {
    session.close_modeled_resources().map_err(fault)?;
    let held = count_usize(session.held(HOLDER).len());
    let os = u64::from(session.os_terminated());
    if session.mode() != Mode::Stopped || held != 0 || os != 0 {
        return Err(silence("closing modeled resources claimed the wrong state"));
    }
    match session.observe_time(LogicalTime(now)).map_err(fault)? {
        Liveness::Stopped {
            modeled_resources_closed: true,
        } => {}
        _ => return Err(silence("stopped trace was still cleaning")),
    }
    Ok(Trace {
        name: "closed".to_owned(),
        origin,
        now,
        mode: "stopped".to_owned(),
        held,
        closed: 1,
        os,
        chunks: vec![Vec::new()],
    })
}

fn stop_at(session: &mut Session, at: u64) -> Result<(), WorldError> {
    let ack = session
        .submit(ControlIntent::Stop, LogicalTime(at))
        .map_err(fault)?;
    if ack.intent != ControlIntent::Stop || ack.at.0 != at || ack.mode != Mode::Stopping {
        return Err(silence("stop did not keep the modeled lease"));
    }
    cleaning_state(session).map(|_| ())
}

fn cleaning_state(session: &Session) -> Result<(u64, u64), WorldError> {
    let held = count_usize(session.held(HOLDER).len());
    let os = u64::from(session.os_terminated());
    if session.mode() != Mode::Stopping || held != 1 || os != 0 {
        return Err(silence(
            "cleanup lost the modeled lease or claimed process death",
        ));
    }
    Ok((held, os))
}

fn observe_until(session: &mut Session, now: u64) -> Result<Vec<Vec<u64>>, WorldError> {
    let mut chunks = Vec::new();
    let mut cursor = None;
    for _ in 0..OBSERVE_PASSES {
        let ticks = one_pass(session, now)?;
        let first = ticks[0];
        let last = ticks[ticks.len() - 1];
        if let Some(previous) = cursor {
            beat_gap(previous, first)?;
        }
        cursor = Some(last);
        let done = last == now;
        chunks.push(ticks);
        if done {
            return Ok(chunks);
        }
    }
    Err(silence(
        "cleanup observation did not reach the requested tick",
    ))
}

fn one_pass(session: &mut Session, now: u64) -> Result<Vec<u64>, WorldError> {
    let live = session.observe_time(LogicalTime(now)).map_err(fault)?;
    let Liveness::Cleaning { beats } = live else {
        return Err(silence("cleanup observation left the cleaning state"));
    };
    let ticks: Vec<u64> = beats.into_iter().map(|beat| beat.0).collect();
    if ticks.is_empty() {
        return Err(silence("cleanup observation emitted no beat"));
    }
    for pair in ticks.windows(2) {
        beat_gap(pair[0], pair[1])?;
    }
    Ok(ticks)
}

fn require_span(chunks: &[Vec<u64>], origin: u64, now: u64) -> Result<(), WorldError> {
    let Some(first) = chunks.first().and_then(|chunk| chunk.first()).copied() else {
        return Err(silence("cleanup observation emitted no beat"));
    };
    let Some(last) = chunks.last().and_then(|chunk| chunk.last()).copied() else {
        return Err(silence("cleanup observation emitted no beat"));
    };
    if first != origin {
        return Err(silence("cleanup beats do not start at the stop"));
    }
    if last != now {
        return Err(silence("cleanup beats do not reach the observation"));
    }
    let total: usize = chunks.iter().map(Vec::len).sum();
    if now.saturating_sub(origin) >= SILENCE && total <= 1 {
        return Err(silence("a long cleanup trace recorded only its final beat"));
    }
    Ok(())
}

fn beat_gap(earlier: u64, later: u64) -> Result<(), WorldError> {
    if later > earlier && within_cleanup_gap(later - earlier) {
        Ok(())
    } else {
        Err(silence("cleanup beats left a logical silence"))
    }
}

fn open_holder() -> Result<Session, WorldError> {
    let unit = ProofUnit {
        id: HOLDER.to_owned(),
        depends_on: Vec::new(),
        capability: "run".to_owned(),
        invalidation_mass: 1,
        learned_cost: 1,
        critical_path_gain: 0,
        novelty_gain: 0,
        resources: vec![ResourceAsk {
            kind: "disk".to_owned(),
            amount: 1,
        }],
    };
    let mut pools = BTreeMap::new();
    pools.insert("disk".to_owned(), 1);
    let mut session =
        Session::new(vec![unit], BTreeSet::from(["run".to_owned()]), pools).map_err(fault)?;
    match session.admit_next(LogicalTime(0)).map_err(fault)? {
        Admit::Admitted { unit } if unit == HOLDER => Ok(session),
        _ => Err(silence("modeled resource was not admitted")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Trace {
    name: String,
    origin: u64,
    now: u64,
    mode: String,
    held: u64,
    closed: u64,
    os: u64,
    chunks: Vec<Vec<u64>>,
}

fn expect_names(traces: &[Trace]) -> Result<(), WorldError> {
    let names: Vec<&str> = traces.iter().map(|trace| trace.name.as_str()).collect();
    if names == ["span", "origin", "long", "refused", "closed"] {
        Ok(())
    } else {
        Err(silence("cleanup measurement omitted a modeled trace"))
    }
}

fn export_trace(traces: &[Trace]) -> String {
    let mut text = String::new();
    for trace in traces {
        for beats in &trace.chunks {
            text.push_str(&format_line(trace, beats));
            text.push('\n');
        }
    }
    text
}

fn format_line(trace: &Trace, beats: &[u64]) -> String {
    let mut line = format!(
        "{} {} {} {} {} {} {}",
        trace.name, trace.origin, trace.now, trace.mode, trace.held, trace.closed, trace.os
    );
    for beat in beats {
        line.push(' ');
        line.push_str(&beat.to_string());
    }
    line
}

fn reopen_trace(exported: &str) -> Result<Vec<Trace>, WorldError> {
    if exported.is_empty() || !exported.ends_with('\n') {
        return Err(silence("cleanup trace export is empty"));
    }
    let mut traces = Vec::new();
    for line in exported.lines() {
        push_line(&mut traces, parse_line(line)?)?;
    }
    for trace in &traces {
        accept_trace(trace)?;
    }
    Ok(traces)
}

fn push_line(traces: &mut Vec<Trace>, row: Trace) -> Result<(), WorldError> {
    if let Some(current) = traces.last_mut()
        && current.name == row.name
    {
        if current.origin != row.origin
            || current.now != row.now
            || current.mode != row.mode
            || current.held != row.held
            || current.closed != row.closed
            || current.os != row.os
            || row.chunks.len() != 1
        {
            return Err(silence("cleanup continuation changed its trace"));
        }
        current.chunks.extend(row.chunks);
        return Ok(());
    }
    traces.push(row);
    Ok(())
}

fn parse_line(line: &str) -> Result<Trace, WorldError> {
    let mut parts = line.split_whitespace();
    let name = token(parts.next())?;
    let origin = parse_u64(parts.next())?;
    let now = parse_u64(parts.next())?;
    let mode = token(parts.next())?;
    let held = parse_u64(parts.next())?;
    let closed = parse_u64(parts.next())?;
    let os = parse_u64(parts.next())?;
    let mut beats = Vec::new();
    for part in parts {
        beats.push(parse_u64(Some(part))?);
    }
    Ok(Trace {
        name,
        origin,
        now,
        mode,
        held,
        closed,
        os,
        chunks: vec![beats],
    })
}

fn accept_trace(trace: &Trace) -> Result<(), WorldError> {
    if trace.os != 0 {
        return Err(silence("cleanup trace claims process death"));
    }
    if trace.mode == "stopped" {
        return accept_stopped(trace);
    }
    if trace.mode != "cleaning" {
        return Err(silence("cleanup trace names an unknown mode"));
    }
    accept_cleaning(trace)
}

fn accept_stopped(trace: &Trace) -> Result<(), WorldError> {
    if trace.held == 0 && trace.closed == 1 && trace.chunks.len() == 1 && trace.chunks[0].is_empty()
    {
        Ok(())
    } else {
        Err(silence(
            "stopped cleanup trace still holds a modeled resource",
        ))
    }
}

fn accept_cleaning(trace: &Trace) -> Result<(), WorldError> {
    if trace.held != 1 || trace.closed != 0 || trace.chunks.is_empty() {
        return Err(silence("cleaning trace does not hold its modeled resource"));
    }
    require_span(&trace.chunks, trace.origin, trace.now)?;
    let mut previous = None;
    for chunk in &trace.chunks {
        let Some(first) = chunk.first().copied() else {
            return Err(silence("cleanup chunk is empty"));
        };
        if let Some(cursor) = previous {
            beat_gap(cursor, first)?;
        }
        for pair in chunk.windows(2) {
            beat_gap(pair[0], pair[1])?;
        }
        previous = chunk.last().copied();
    }
    Ok(())
}

fn record(
    traces: &[Trace],
    exported: &str,
    refused: u64,
    effects: u64,
    subprocesses: u64,
) -> Datum {
    let chunks = traces.iter().map(|trace| trace.chunks.len()).sum::<usize>();
    let beats = traces
        .iter()
        .flat_map(|trace| &trace.chunks)
        .map(Vec::len)
        .sum::<usize>();
    let os_terminated = traces.iter().any(|trace| trace.os != 0);
    Datum::Node {
        tag: Symbol::qualified("world", "cleanup-beats-v1"),
        fields: vec![
            (Symbol::new("command"), Datum::String("cleanup".to_owned())),
            (
                Symbol::new("traces"),
                query::count_datum(count_usize(traces.len())),
            ),
            (
                Symbol::new("chunks"),
                query::count_datum(count_usize(chunks)),
            ),
            (Symbol::new("beats"), query::count_datum(count_usize(beats))),
            (Symbol::new("refused"), query::count_datum(refused)),
            (Symbol::new("effects"), query::count_datum(effects)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(subprocesses),
            ),
            (Symbol::new("os-terminated"), Datum::Bool(os_terminated)),
            (Symbol::new("reopened"), Datum::Bool(true)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("trace"), Datum::String(exported.to_owned())),
        ],
    }
}

fn token(value: Option<&str>) -> Result<String, WorldError> {
    value
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| silence("cleanup trace line is short"))
}

fn parse_u64(token: Option<&str>) -> Result<u64, WorldError> {
    token
        .ok_or_else(|| silence("cleanup trace line is short"))?
        .parse()
        .map_err(|_| silence("cleanup trace number is not decimal"))
}

fn count_usize(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn silence(text: &str) -> WorldError {
    WorldError::Qualification(text.to_owned())
}

fn fault(error: ScheduleError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

#[cfg(test)]
#[path = "cleanup_beats_tests.rs"]
mod tests;
