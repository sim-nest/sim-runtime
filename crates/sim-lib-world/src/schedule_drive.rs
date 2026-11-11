// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published scheduler.
//!
//! Each case calls the published frontier and session functions. This module
//! does not admit a worker, and it does not treat modeled closure as process
//! death.

use std::collections::{BTreeMap, BTreeSet};

use sim_lib_roadmap_runner::{
    Admit, ControlIntent, FailureFrontier, Liveness, LogicalTime, Mode, ProofOutcome, ProofUnit,
    ResourceAsk, ScheduleError, Session, WakeCause, failure_frontier, legal_order, resume_frontier,
};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) enum Fault {
    Bound,
    Refused,
}

impl Fault {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Refused => "schedule-refused",
        }
    }
}

pub(crate) fn frontier() -> Result<Law, Fault> {
    let mut invokes = 0;
    let failed = ProofOutcome::Failed;
    let units = vec![
        plain("b", &[], "run", 0, 0, 0, 0),
        plain("a", &[], "run", 0, 0, 0, 0),
        plain("c", &[], "run", 0, 0, 0, 0),
    ];
    let mut reversed = units.clone();
    reversed.reverse();
    let skip = BTreeSet::new();
    let caps = caps();
    let forward_map = outcomes(&[("c", failed), ("a", failed), ("b", failed)]);
    let reverse_map = outcomes(&[("b", failed), ("a", failed), ("c", failed)]);
    let forward = frontier_of(&units, &forward_map, &caps, &skip, 10, &mut invokes)?;
    let backward = frontier_of(&reversed, &reverse_map, &caps, &skip, 10, &mut invokes)?;
    let partial_forward = frontier_of(&units, &forward_map, &caps, &skip, 1, &mut invokes)?;
    let partial_backward = frontier_of(&reversed, &reverse_map, &caps, &skip, 1, &mut invokes)?;
    if forward.failures != backward.failures
        || partial_forward.failures != partial_backward.failures
        || forward.continuation.is_some()
        || class_text(&partial_forward) != "a:proof-failed"
    {
        return Err(Fault::Bound);
    }
    let ranked = vec![
        plain("a", &[], "run", 10, 2, 0, 0),
        plain("c", &[], "run", 1, 0, 4, 1),
        plain("b", &[], "run", 9, 3, 0, 0),
        plain("d", &["b"], "run", 100, 1, 0, 0),
        plain("z", &[], "other", 1, 1, 0, 0),
    ];
    invokes += 1;
    let order = legal_order(&ranked, &BTreeMap::new(), &caps, &BTreeSet::new()).map_err(refused)?;
    if order != ["a", "c", "b"] {
        return Err(Fault::Bound);
    }
    let cheap = vec![
        plain("a", &[], "run", 100, 0, 0, 0),
        plain("b", &[], "run", 1, 1, 0, 0),
    ];
    invokes += 1;
    let zero = legal_order(&cheap, &BTreeMap::new(), &caps, &BTreeSet::new()).map_err(refused)?;
    if zero != ["b", "a"] {
        return Err(Fault::Bound);
    }
    let cycled = vec![
        plain("a", &["b"], "run", 0, 0, 0, 0),
        plain("b", &["a"], "run", 0, 0, 0, 0),
        plain("m", &["ghost"], "run", 0, 0, 0, 0),
    ];
    let mut cycled_back = cycled.clone();
    cycled_back.reverse();
    let empty = BTreeMap::new();
    let cycle_forward = frontier_of(&cycled, &empty, &caps, &skip, 10, &mut invokes)?;
    let cycle_backward = frontier_of(&cycled_back, &empty, &caps, &skip, 10, &mut invokes)?;
    let cycles = class_text(&cycle_forward);
    if cycle_forward.failures != cycle_backward.failures
        || cycles != "a:dependency-cycle,b:dependency-cycle,m:missing-dependency"
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("same 1 partial a:proof-failed order a,c,b zero b,a cycles {cycles}"),
        invokes,
    })
}

pub(crate) fn resume() -> Result<Law, Fault> {
    let mut invokes = 0;
    let units = vec![
        plain("a", &[], "run", 0, 0, 0, 0),
        plain("b", &[], "run", 0, 0, 0, 0),
        plain("c", &[], "run", 0, 0, 0, 0),
    ];
    let found = outcomes(&[
        ("a", ProofOutcome::Failed),
        ("b", ProofOutcome::Failed),
        ("c", ProofOutcome::Failed),
    ]);
    let skip = BTreeSet::new();
    let caps = caps();
    let full = frontier_of(&units, &found, &caps, &skip, 10, &mut invokes)?;
    let first = frontier_of(&units, &found, &caps, &skip, 1, &mut invokes)?;
    let text = first
        .continuation
        .as_ref()
        .ok_or(Fault::Bound)?
        .text
        .clone();
    invokes += 1;
    let second = resume_frontier(&units, &found, &caps, &skip, &text, 1).map_err(refused)?;
    let third_text = second
        .continuation
        .as_ref()
        .ok_or(Fault::Bound)?
        .text
        .clone();
    invokes += 1;
    let third = resume_frontier(&units, &found, &caps, &skip, &third_text, 10).map_err(refused)?;
    if third.failures != full.failures || third.continuation.is_some() || full.failures.is_empty() {
        return Err(Fault::Bound);
    }
    let failures = full.failures.len();
    if failures != 3 {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("failures {failures} open 0 same 1"),
        invokes,
    })
}

pub(crate) fn wait() -> Result<Law, Fault> {
    let mut invokes = 0;
    let mut scheduler = session(
        vec![
            asked("a", 10, &[("cpu", 1), ("gpu", 1)]),
            asked("b", 1, &[("disk", 1)]),
        ],
        &[("cpu", 1), ("gpu", 0), ("disk", 1)],
    )?;
    invokes += 1;
    let admitted = scheduler.admit_next(LogicalTime(1)).map_err(refused)?;
    if !matches!(admitted, Admit::Admitted { ref unit } if unit == "b") {
        return Err(Fault::Bound);
    }
    let held_a = scheduler.held("a").len();
    let cpu = scheduler.pool("cpu");
    let held_b = scheduler.held("b").len();
    let disk_kind = scheduler
        .held("b")
        .first()
        .map(|lease| lease.kind.clone())
        .unwrap_or_default();
    let disk = scheduler.pool("disk");
    invokes += 1;
    let live = scheduler.observe_time(LogicalTime(1)).map_err(refused)?;
    let Liveness::Waiting { unit, kind, .. } = live else {
        return Err(Fault::Bound);
    };
    if held_a != 0
        || cpu != 1
        || held_b != 1
        || disk_kind != "disk"
        || disk != 0
        || unit != "a"
        || kind != "gpu"
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("admitted b held-a {held_a} cpu {cpu} disk {disk} waiting {unit} {kind}"),
        invokes,
    })
}

pub(crate) fn control() -> Result<Law, Fault> {
    let mut invokes = 0;
    let units = (0..8)
        .map(|index| asked(&format!("q{index}"), 1, &[]))
        .collect();
    let mut scheduler = session(units, &[])?;
    for _ in 0..8 {
        invokes += 1;
        if !matches!(
            scheduler.admit_next(LogicalTime(0)).map_err(refused)?,
            Admit::Admitted { .. }
        ) {
            return Err(Fault::Bound);
        }
    }
    let queued = scheduler.work_len();
    if queued != 8 || !scheduler.control_log().is_empty() {
        return Err(Fault::Bound);
    }
    invokes += 1;
    let ack = scheduler
        .submit(ControlIntent::Stop, LogicalTime(2))
        .map_err(refused)?;
    if ack.mode != Mode::Stopped
        || scheduler.work_len() != 0
        || scheduler.control_log().len() != 1
        || scheduler.control_log()[0].intent != ControlIntent::Stop
        || scheduler.os_terminated()
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("queued {queued} stopped 0 log stop os 0"),
        invokes,
    })
}

pub(crate) fn liveness() -> Result<Law, Fault> {
    let mut invokes = 0;
    let mut stalled = session(vec![asked("s0", 1, &[])], &[])?;
    invokes += 1;
    stalled.admit_next(LogicalTime(0)).map_err(refused)?;
    invokes += 1;
    let live = stalled.observe_time(LogicalTime(30)).map_err(refused)?;
    let Liveness::Stalled { unit, since } = live else {
        return Err(Fault::Bound);
    };
    invokes += 1;
    let failures = stalled.failure_set(10).map_err(refused)?.failures.len();
    let work = stalled.work_len();
    invokes += 1;
    let idle = matches!(
        stalled.admit_next(LogicalTime(30)).map_err(refused)?,
        Admit::Idle
    );
    if unit != "s0" || since != LogicalTime(0) || failures != 0 || work != 1 || !idle {
        return Err(Fault::Bound);
    }
    let mut cleaning = session(vec![asked("a", 1, &[("disk", 1)])], &[("disk", 1)])?;
    invokes += 1;
    cleaning.admit_next(LogicalTime(0)).map_err(refused)?;
    invokes += 1;
    let ack = cleaning
        .submit(ControlIntent::Stop, LogicalTime(0))
        .map_err(refused)?;
    if ack.mode != Mode::Stopping || cleaning.held("a").len() != 1 {
        return Err(Fault::Bound);
    }
    invokes += 1;
    let Liveness::Cleaning { beats } = cleaning.observe_time(LogicalTime(23)).map_err(refused)?
    else {
        return Err(Fault::Bound);
    };
    let ticks: Vec<u64> = beats.iter().map(|beat| beat.0).collect();
    let gap_ok =
        ticks.first().copied() == Some(0) && ticks.windows(2).all(|pair| pair[1] - pair[0] < 5);
    if !gap_ok {
        return Err(Fault::Bound);
    }
    let beat_count = ticks.len();
    invokes += 1;
    cleaning.close_modeled_resources().map_err(refused)?;
    invokes += 1;
    let closed = matches!(
        cleaning.observe_time(LogicalTime(23)).map_err(refused)?,
        Liveness::Stopped {
            modeled_resources_closed: true
        }
    );
    if !closed || cleaning.os_terminated() || cleaning.mode() != Mode::Stopped {
        return Err(Fault::Bound);
    }
    let mut quiet = session(Vec::new(), &[])?;
    invokes += 1;
    let Liveness::Quiescent { wake_on } = quiet.observe_time(LogicalTime(0)).map_err(refused)?
    else {
        return Err(Fault::Bound);
    };
    let wake = wake_on.iter().map(cause_name).collect::<Vec<_>>().join(",");
    if wake != "resource-offer,control,fact-refresh" {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!(
            "stalled {unit} since {} failures {failures} work {work} idle 1 beats {beat_count} closed 1 os 0 wake {wake}",
            since.0
        ),
        invokes,
    })
}

fn frontier_of(
    units: &[ProofUnit],
    outcomes: &BTreeMap<String, ProofOutcome>,
    capabilities: &BTreeSet<String>,
    skip: &BTreeSet<String>,
    budget: u32,
    invokes: &mut u64,
) -> Result<FailureFrontier, Fault> {
    *invokes += 1;
    failure_frontier(units, outcomes, capabilities, skip, budget).map_err(refused)
}

fn class_text(frontier: &FailureFrontier) -> String {
    frontier
        .failures
        .iter()
        .map(|failure| format!("{}:{}", failure.unit, failure.class))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

fn session(units: Vec<ProofUnit>, pools: &[(&str, u64)]) -> Result<Session, Fault> {
    Session::new(
        units,
        caps(),
        pools
            .iter()
            .map(|(kind, amount)| ((*kind).to_owned(), *amount))
            .collect(),
    )
    .map_err(refused)
}

fn caps() -> BTreeSet<String> {
    BTreeSet::from(["run".to_owned()])
}

fn outcomes(pairs: &[(&str, ProofOutcome)]) -> BTreeMap<String, ProofOutcome> {
    pairs
        .iter()
        .map(|(id, outcome)| ((*id).to_owned(), *outcome))
        .collect()
}

fn plain(
    id: &str,
    depends_on: &[&str],
    capability: &str,
    mass: u64,
    cost: u64,
    critical: u64,
    novelty: u64,
) -> ProofUnit {
    ProofUnit {
        id: id.to_owned(),
        depends_on: depends_on.iter().map(|dep| (*dep).to_owned()).collect(),
        capability: capability.to_owned(),
        invalidation_mass: mass,
        learned_cost: cost,
        critical_path_gain: critical,
        novelty_gain: novelty,
        resources: Vec::new(),
    }
}

fn asked(id: &str, mass: u64, resources: &[(&str, u64)]) -> ProofUnit {
    ProofUnit {
        id: id.to_owned(),
        depends_on: Vec::new(),
        capability: "run".to_owned(),
        invalidation_mass: mass,
        learned_cost: 1,
        critical_path_gain: 0,
        novelty_gain: 0,
        resources: resources
            .iter()
            .map(|(kind, amount)| ResourceAsk {
                kind: (*kind).to_owned(),
                amount: *amount,
            })
            .collect(),
    }
}

fn cause_name(cause: &WakeCause) -> &'static str {
    match cause {
        WakeCause::ResourceOffer => "resource-offer",
        WakeCause::Control => "control",
        WakeCause::FactRefresh => "fact-refresh",
    }
}

fn refused(error: ScheduleError) -> Fault {
    let _ = error;
    Fault::Refused
}
