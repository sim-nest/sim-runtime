// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One row for each selected portable phase.

use super::{Gate, Ground};

pub(super) const GATES: &[Gate] = &[
    Gate {
        leaf: "phase/00",
        ground: Ground::Gap("no-owner-result"),
    },
    Gate {
        leaf: "phase/01",
        ground: Ground::Bind,
    },
    Gate {
        leaf: "phase/02",
        ground: Ground::Journal,
    },
    Gate {
        leaf: "phase/03",
        ground: Ground::Normalize,
    },
    Gate {
        leaf: "phase/04",
        ground: Ground::Operate,
    },
    Gate {
        leaf: "phase/05",
        ground: Ground::Recover,
    },
    Gate {
        leaf: "phase/06",
        ground: Ground::Project,
    },
    Gate {
        leaf: "phase/07",
        ground: Ground::Assay,
    },
    Gate {
        leaf: "phase/08",
        ground: Ground::Plan,
    },
    Gate {
        leaf: "phase/09",
        ground: Ground::Observe,
    },
    Gate {
        leaf: "phase/10",
        ground: Ground::Gap("host-read"),
    },
    Gate {
        leaf: "phase/11",
        ground: Ground::Foreign,
    },
    Gate {
        leaf: "phase/12",
        ground: Ground::Evidence,
    },
    Gate {
        leaf: "phase/13",
        ground: Ground::Schedule,
    },
    Gate {
        leaf: "phase/14",
        ground: Ground::Gap("circular"),
    },
    Gate {
        leaf: "phase/15",
        ground: Ground::Converge,
    },
    Gate {
        leaf: "phase/16",
        ground: Ground::Fixed,
    },
    Gate {
        leaf: "phase/17",
        ground: Ground::Transact,
    },
    Gate {
        leaf: "phase/18",
        ground: Ground::Land,
    },
    Gate {
        leaf: "phase/19",
        ground: Ground::Service,
    },
    Gate {
        leaf: "phase/20",
        ground: Ground::Release,
    },
    Gate {
        leaf: "phase/21",
        ground: Ground::Deliver,
    },
    Gate {
        leaf: "phase/22",
        ground: Ground::Canary,
    },
    Gate {
        leaf: "phase/23",
        ground: Ground::Work,
    },
    Gate {
        leaf: "phase/24",
        ground: Ground::Author,
    },
    Gate {
        leaf: "phase/25",
        ground: Ground::Conduct,
    },
    Gate {
        leaf: "phase/26",
        ground: Ground::Qualify,
    },
    Gate {
        leaf: "phase/27",
        ground: Ground::Gap("held"),
    },
    Gate {
        leaf: "phase/28",
        ground: Ground::Gap("unimplemented"),
    },
];
