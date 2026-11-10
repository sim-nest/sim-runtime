// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent expected sets for the frozen six-delta assay.
//!
//! These rows are the oracle. They are not the candidate's prediction and
//! they are not computed by `judge`. A mutant matches only when its affected
//! set, changed facts, and revision agree with this table.

use std::collections::{BTreeMap, BTreeSet};

use sim_incremental_core::projection::{FactId, FederatedClosure};
use sim_kernel::Datum;

use crate::WorldError;
use crate::assay::AssayObservation;

/// One delta's expected prediction.
pub(crate) struct Expected {
    /// Delta name.
    pub(crate) delta: &'static str,
    /// Facts whose semantic bytes change.
    pub(crate) changed: &'static [&'static str],
    /// Conclusions the sealed graph must mark affected.
    pub(crate) affected: &'static [&'static str],
    /// Unaffected conclusions, in basis points.
    pub(crate) basis: u16,
    /// Order-only revision count.
    pub(crate) revision: u64,
}

const EXPECTED: &[Expected] = &[
    Expected {
        delta: "D1",
        changed: &[],
        affected: &[],
        basis: 10_000,
        revision: 0,
    },
    Expected {
        delta: "D2",
        changed: &["change/private-implementation"],
        affected: &["conclusion/01", "conclusion/02"],
        basis: 9_500,
        revision: 0,
    },
    Expected {
        delta: "D3",
        changed: &["change/public-api"],
        affected: &[
            "conclusion/03",
            "conclusion/04",
            "conclusion/05",
            "conclusion/06",
            "conclusion/07",
            "conclusion/08",
            "conclusion/09",
            "conclusion/10",
        ],
        basis: 8_000,
        revision: 0,
    },
    Expected {
        delta: "D4",
        changed: &[
            "change/control-pins",
            "change/head-a",
            "change/head-b",
            "change/head-c",
        ],
        affected: &[
            "conclusion/11",
            "conclusion/12",
            "conclusion/13",
            "conclusion/14",
            "conclusion/15",
            "conclusion/16",
            "conclusion/17",
            "conclusion/18",
            "conclusion/19",
            "conclusion/20",
            "conclusion/21",
            "conclusion/22",
        ],
        basis: 7_000,
        revision: 0,
    },
    Expected {
        delta: "D5",
        changed: &["change/checker-alpha", "change/proof-code-alpha"],
        affected: &[
            "conclusion/23",
            "conclusion/24",
            "conclusion/25",
            "conclusion/26",
        ],
        basis: 9_000,
        revision: 0,
    },
    Expected {
        delta: "D6",
        changed: &[],
        affected: &[],
        basis: 10_000,
        revision: 1,
    },
];

/// The frozen expected rows.
#[cfg(test)]
pub(crate) fn expected() -> &'static [Expected] {
    EXPECTED
}

/// Whether the candidate prediction agrees with the oracle.
pub(crate) fn matches(observation: &AssayObservation) -> bool {
    observation.denominator == 40
        && observation.rows.len() == EXPECTED.len()
        && observation
            .rows
            .iter()
            .zip(EXPECTED)
            .all(|(row, expected)| {
                row.delta == expected.delta
                    && row.unaffected_basis_points == expected.basis
                    && row.revision_delta == expected.revision
                    && !row.carried_state
                    && same(&row.changed, expected.changed)
                    && same(&row.affected, expected.affected)
            })
}

fn same<T: ToString>(values: &BTreeSet<T>, expected: &[&str]) -> bool {
    let got: Vec<String> = values.iter().map(ToString::to_string).collect();
    got == expected
        .iter()
        .map(|item| (*item).to_owned())
        .collect::<Vec<_>>()
}

/// One commissioned assay conclusion.
///
/// The table is the sensitivity assay. It is not the portable proof
/// denominator. Cold assay execution checks the sealed graph against these
/// rows and runs each conclusion through the roadmap proof catalog.
pub(crate) struct Obligation {
    /// Portable requirement this leaf discharges.
    pub(crate) requirement: &'static str,
    /// Catalog leaf name.
    pub(crate) leaf: &'static str,
    /// Declared facts in universe order.
    pub(crate) facts: &'static [&'static str],
}

const OBLIGATIONS: &[Obligation] = &[
    Obligation {
        requirement: "assay/conclusion/01",
        leaf: "conclusion/01",
        facts: &["stable/01", "change/private-implementation"],
    },
    Obligation {
        requirement: "assay/conclusion/02",
        leaf: "conclusion/02",
        facts: &["stable/02", "change/private-implementation"],
    },
    Obligation {
        requirement: "assay/conclusion/03",
        leaf: "conclusion/03",
        facts: &["stable/03", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/04",
        leaf: "conclusion/04",
        facts: &["stable/04", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/05",
        leaf: "conclusion/05",
        facts: &["stable/05", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/06",
        leaf: "conclusion/06",
        facts: &["stable/06", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/07",
        leaf: "conclusion/07",
        facts: &["stable/07", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/08",
        leaf: "conclusion/08",
        facts: &["stable/08", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/09",
        leaf: "conclusion/09",
        facts: &["stable/09", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/10",
        leaf: "conclusion/10",
        facts: &["stable/10", "change/public-api"],
    },
    Obligation {
        requirement: "assay/conclusion/11",
        leaf: "conclusion/11",
        facts: &["stable/11", "change/head-a"],
    },
    Obligation {
        requirement: "assay/conclusion/12",
        leaf: "conclusion/12",
        facts: &["stable/12", "change/head-a"],
    },
    Obligation {
        requirement: "assay/conclusion/13",
        leaf: "conclusion/13",
        facts: &["stable/13", "change/head-a"],
    },
    Obligation {
        requirement: "assay/conclusion/14",
        leaf: "conclusion/14",
        facts: &["stable/14", "change/head-b"],
    },
    Obligation {
        requirement: "assay/conclusion/15",
        leaf: "conclusion/15",
        facts: &["stable/15", "change/head-b"],
    },
    Obligation {
        requirement: "assay/conclusion/16",
        leaf: "conclusion/16",
        facts: &["stable/16", "change/head-b"],
    },
    Obligation {
        requirement: "assay/conclusion/17",
        leaf: "conclusion/17",
        facts: &["stable/17", "change/head-c"],
    },
    Obligation {
        requirement: "assay/conclusion/18",
        leaf: "conclusion/18",
        facts: &["stable/18", "change/head-c"],
    },
    Obligation {
        requirement: "assay/conclusion/19",
        leaf: "conclusion/19",
        facts: &["stable/19", "change/head-c"],
    },
    Obligation {
        requirement: "assay/conclusion/20",
        leaf: "conclusion/20",
        facts: &["stable/20", "change/control-pins"],
    },
    Obligation {
        requirement: "assay/conclusion/21",
        leaf: "conclusion/21",
        facts: &["stable/21", "change/control-pins"],
    },
    Obligation {
        requirement: "assay/conclusion/22",
        leaf: "conclusion/22",
        facts: &["stable/22", "change/control-pins"],
    },
    Obligation {
        requirement: "assay/conclusion/23",
        leaf: "conclusion/23",
        facts: &["stable/23", "change/proof-code-alpha"],
    },
    Obligation {
        requirement: "assay/conclusion/24",
        leaf: "conclusion/24",
        facts: &["stable/24", "change/proof-code-alpha"],
    },
    Obligation {
        requirement: "assay/conclusion/25",
        leaf: "conclusion/25",
        facts: &["stable/25", "change/checker-alpha"],
    },
    Obligation {
        requirement: "assay/conclusion/26",
        leaf: "conclusion/26",
        facts: &["stable/26", "change/checker-alpha"],
    },
    Obligation {
        requirement: "assay/conclusion/27",
        leaf: "conclusion/27",
        facts: &["stable/27"],
    },
    Obligation {
        requirement: "assay/conclusion/28",
        leaf: "conclusion/28",
        facts: &["stable/28"],
    },
    Obligation {
        requirement: "assay/conclusion/29",
        leaf: "conclusion/29",
        facts: &["stable/29"],
    },
    Obligation {
        requirement: "assay/conclusion/30",
        leaf: "conclusion/30",
        facts: &["stable/30"],
    },
    Obligation {
        requirement: "assay/conclusion/31",
        leaf: "conclusion/31",
        facts: &["stable/31"],
    },
    Obligation {
        requirement: "assay/conclusion/32",
        leaf: "conclusion/32",
        facts: &["stable/32"],
    },
    Obligation {
        requirement: "assay/conclusion/33",
        leaf: "conclusion/33",
        facts: &["stable/33"],
    },
    Obligation {
        requirement: "assay/conclusion/34",
        leaf: "conclusion/34",
        facts: &["stable/34"],
    },
    Obligation {
        requirement: "assay/conclusion/35",
        leaf: "conclusion/35",
        facts: &["stable/35"],
    },
    Obligation {
        requirement: "assay/conclusion/36",
        leaf: "conclusion/36",
        facts: &["stable/36"],
    },
    Obligation {
        requirement: "assay/conclusion/37",
        leaf: "conclusion/37",
        facts: &["stable/37"],
    },
    Obligation {
        requirement: "assay/conclusion/38",
        leaf: "conclusion/38",
        facts: &["stable/38"],
    },
    Obligation {
        requirement: "assay/conclusion/39",
        leaf: "conclusion/39",
        facts: &["stable/39"],
    },
    Obligation {
        requirement: "assay/conclusion/40",
        leaf: "conclusion/40",
        facts: &["stable/40"],
    },
];

/// Leaf names of the commissioned assay, in table order.
#[cfg(test)]
pub(crate) fn obligation_leaves() -> Vec<String> {
    OBLIGATIONS
        .iter()
        .map(|obligation| obligation.leaf.to_owned())
        .collect()
}

/// Rows of the commissioned assay, in table order.
pub(crate) fn oracle_rows() -> &'static [Obligation] {
    OBLIGATIONS
}

/// Facts the assay catalog executes, after the sealed graph matches this table.
pub(crate) fn require_union(
    closure: &FederatedClosure,
) -> Result<BTreeMap<String, Vec<FactId>>, WorldError> {
    if OBLIGATIONS.is_empty() {
        return Err(WorldError::Qualification(
            "commissioned assay is empty".to_owned(),
        ));
    }
    let universe = crate::assay::universe()?;
    let mut facts = BTreeMap::new();
    let mut expected = BTreeSet::new();
    for obligation in OBLIGATIONS {
        if obligation.requirement.is_empty() || obligation.facts.is_empty() {
            return Err(WorldError::Qualification(format!(
                "unavailable-proof {}",
                obligation.leaf
            )));
        }
        expected.insert(obligation.leaf);
        let conclusion =
            sim_incremental_core::projection::ConclusionId::new(obligation.leaf.to_owned())?;
        let sealed = universe
            .iter()
            .filter(|fact| closure.depends_on(&conclusion, fact))
            .map(|fact| fact.as_str())
            .collect::<Vec<_>>();
        if sealed != obligation.facts {
            return Err(WorldError::Qualification(format!(
                "obligation {} does not match the sealed proof {}",
                obligation.requirement, obligation.leaf
            )));
        }
        let declared = obligation
            .facts
            .iter()
            .map(|fact| Ok(FactId::new((*fact).to_owned())?))
            .collect::<Result<Vec<_>, WorldError>>()?;
        facts.insert(obligation.leaf.to_owned(), declared);
    }
    for conclusion in closure.conclusions() {
        if !expected.contains(conclusion.as_str()) {
            return Err(WorldError::Qualification(format!(
                "sealed proof {} is outside the commissioned assay",
                conclusion.as_str()
            )));
        }
    }
    Ok(facts)
}

/// Rows that bind postimages, the sealed graph, policy, proof code, and this oracle.
///
/// The runner journal, its record codec, its record law, its execution record,
/// its secret check, its causal apply, the runner resolver lock, the proof
/// view, the proof record, the world resolver lock, the explanation renderer,
/// and the observation-count renderer are part of the same subject. A change
/// to any of them changes the freeze.
pub(crate) fn closure_rows(graph: &[String]) -> Vec<Datum> {
    let mut rows = Vec::new();
    for line in graph {
        rows.push(Datum::String(line.clone()));
    }
    rows.push(Datum::String("projection/trusted-native-v1".to_owned()));
    rows.push(Datum::String(include_str!("prove_leaf.rs").to_owned()));
    rows.push(Datum::String(include_str!("prove_run.rs").to_owned()));
    rows.push(Datum::String(include_str!("prove.rs").to_owned()));
    rows.push(Datum::String(include_str!("prove_view.rs").to_owned()));
    rows.push(Datum::String(include_str!("prove_record.rs").to_owned()));
    rows.push(Datum::String(include_str!("assay.rs").to_owned()));
    rows.push(Datum::String(include_str!("assay_mutant.rs").to_owned()));
    rows.push(Datum::String(include_str!("provider.rs").to_owned()));
    rows.push(Datum::String(include_str!("assay_oracle.rs").to_owned()));
    rows.push(Datum::String(include_str!("product.rs").to_owned()));
    rows.push(Datum::String(include_str!("query.rs").to_owned()));
    rows.push(Datum::String(
        include_str!("../freeze/incremental-graph.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-proof.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-journal.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-codec.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-record-law.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-model.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-journal-util.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runner-causal.rs").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/agent-net-Cargo.lock").to_owned(),
    ));
    rows.push(Datum::String(
        include_str!("../freeze/runtime-Cargo.lock").to_owned(),
    ));
    rows
}
