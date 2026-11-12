// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted owner table for the loaded manifest command.
//!
//! A public pin must be present and equal the observed head. A private
//! pin, an escaped dependency path, two revisions of one package, two
//! owners of one package, an unowned package, a dependency cycle, an
//! empty source, and a Linux row refuse. A coherent table reports the
//! owners, packages, and edges it counted. The walk does not read a
//! checkout.

use std::collections::{BTreeMap, BTreeSet};

const PIN_ALPHA: &str = "pin-alpha";
const PIN_BETA: &str = "pin-beta";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visibility {
    Public,
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub name: &'static str,
    pub visibility: Visibility,
    pub pin: &'static str,
    pub observed: &'static str,
    pub linux: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Package {
    pub name: &'static str,
    pub owner: &'static str,
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub from: &'static str,
    pub to: &'static str,
    pub revision: &'static str,
    pub path: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Table {
    pub owners: &'static [Owner],
    pub packages: &'static [Package],
    pub edges: &'static [Edge],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Census {
    pub owners: u64,
    pub packages: u64,
    pub edges: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    code: &'static str,
}

impl Fault {
    pub(crate) fn code(self) -> &'static str {
        self.code
    }
}

fn fault(code: &'static str) -> Fault {
    Fault { code }
}

const OWNERS: &[Owner] = &[
    Owner {
        name: "alpha",
        visibility: Visibility::Public,
        pin: PIN_ALPHA,
        observed: PIN_ALPHA,
        linux: false,
    },
    Owner {
        name: "beta",
        visibility: Visibility::Public,
        pin: PIN_BETA,
        observed: PIN_BETA,
        linux: false,
    },
    Owner {
        name: "held",
        visibility: Visibility::Private,
        pin: "",
        observed: "",
        linux: false,
    },
];

const PACKAGES: &[Package] = &[
    Package {
        name: "core",
        owner: "alpha",
        source: "crates/core",
    },
    Package {
        name: "codec",
        owner: "beta",
        source: "crates/codec",
    },
    Package {
        name: "note",
        owner: "held",
        source: "notes/held",
    },
];

const EDGES: &[Edge] = &[Edge {
    from: "codec",
    to: "core",
    revision: PIN_ALPHA,
    path: "crates/core",
}];

pub(crate) const OWNER: Table = Table {
    owners: OWNERS,
    packages: PACKAGES,
    edges: EDGES,
};

const DRIFT_OWNERS: &[Owner] = &[
    Owner {
        name: "alpha",
        visibility: Visibility::Public,
        pin: PIN_ALPHA,
        observed: "pin-alpha-other",
        linux: false,
    },
    OWNERS[1],
    OWNERS[2],
];

const MISSING_OWNERS: &[Owner] = &[
    Owner {
        name: "alpha",
        visibility: Visibility::Public,
        pin: "",
        observed: PIN_ALPHA,
        linux: false,
    },
    OWNERS[1],
    OWNERS[2],
];

const PRIVATE_OWNERS: &[Owner] = &[
    OWNERS[0],
    OWNERS[1],
    Owner {
        name: "held",
        visibility: Visibility::Private,
        pin: "pin-held",
        observed: "",
        linux: false,
    },
];

const ESCAPE_EDGES: &[Edge] = &[Edge {
    from: "codec",
    to: "core",
    revision: PIN_ALPHA,
    path: "../outside/core",
}];

const SPLIT_EDGES: &[Edge] = &[
    Edge {
        from: "codec",
        to: "core",
        revision: PIN_ALPHA,
        path: "crates/core",
    },
    Edge {
        from: "note",
        to: "core",
        revision: PIN_BETA,
        path: "crates/core",
    },
];

const OVERLAP_PACKAGES: &[Package] = &[
    Package {
        name: "core",
        owner: "alpha",
        source: "crates/core",
    },
    Package {
        name: "core",
        owner: "beta",
        source: "crates/other",
    },
    PACKAGES[2],
];

const MISSING_EDGES: &[Edge] = &[Edge {
    from: "codec",
    to: "ghost",
    revision: PIN_ALPHA,
    path: "crates/ghost",
}];

const CYCLE_EDGES: &[Edge] = &[
    Edge {
        from: "codec",
        to: "core",
        revision: PIN_ALPHA,
        path: "crates/core",
    },
    Edge {
        from: "core",
        to: "codec",
        revision: PIN_ALPHA,
        path: "crates/codec",
    },
];

const LINUX_OWNERS: &[Owner] = &[
    Owner {
        name: "alpha",
        visibility: Visibility::Public,
        pin: PIN_ALPHA,
        observed: PIN_ALPHA,
        linux: true,
    },
    OWNERS[1],
    OWNERS[2],
];

const FABRICATED_PACKAGES: &[Package] = &[
    Package {
        name: "core",
        owner: "alpha",
        source: "",
    },
    PACKAGES[1],
    PACKAGES[2],
];

pub(crate) fn table(case: &str) -> Option<Table> {
    Some(match case {
        "owner" => OWNER,
        "pin-drift" => Table {
            owners: DRIFT_OWNERS,
            packages: PACKAGES,
            edges: EDGES,
        },
        "missing-pin" => Table {
            owners: MISSING_OWNERS,
            packages: PACKAGES,
            edges: EDGES,
        },
        "private-pin" => Table {
            owners: PRIVATE_OWNERS,
            packages: PACKAGES,
            edges: EDGES,
        },
        "escape" => Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: ESCAPE_EDGES,
        },
        "split-source" => Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: SPLIT_EDGES,
        },
        "overlap" => Table {
            owners: OWNERS,
            packages: OVERLAP_PACKAGES,
            edges: EDGES,
        },
        "missing-owner" => Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: MISSING_EDGES,
        },
        "cycle" => Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: CYCLE_EDGES,
        },
        "linux-target" => Table {
            owners: LINUX_OWNERS,
            packages: PACKAGES,
            edges: EDGES,
        },
        "fabricated" => Table {
            owners: OWNERS,
            packages: FABRICATED_PACKAGES,
            edges: EDGES,
        },
        _ => return None,
    })
}

pub(crate) fn admit(table: &Table) -> Result<Census, Fault> {
    if table.owners.iter().any(|owner| owner.linux) {
        return Err(fault("linux-target"));
    }
    for owner in table.owners {
        match owner.visibility {
            Visibility::Public if owner.pin.is_empty() => return Err(fault("missing-pin")),
            Visibility::Public if owner.pin != owner.observed => return Err(fault("pin-drift")),
            Visibility::Private if !owner.pin.is_empty() => return Err(fault("private-pin")),
            Visibility::Public | Visibility::Private => {}
        }
    }
    if table.edges.iter().any(|edge| escapes(edge.path)) {
        return Err(fault("escape"));
    }
    if table
        .packages
        .iter()
        .any(|package| package.source.is_empty())
    {
        return Err(fault("fabricated"));
    }
    if overlapped(table) {
        return Err(fault("overlap"));
    }
    if table.owners.is_empty() || table.packages.is_empty() || unowned(table) {
        return Err(fault("missing-owner"));
    }
    if split(table.edges) {
        return Err(fault("split-source"));
    }
    if cyclic(table.edges) {
        return Err(fault("cycle"));
    }
    Ok(Census {
        owners: u64::try_from(table.owners.len()).unwrap_or(u64::MAX),
        packages: u64::try_from(table.packages.len()).unwrap_or(u64::MAX),
        edges: u64::try_from(table.edges.len()).unwrap_or(u64::MAX),
    })
}

fn escapes(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|part| part.is_empty() || part == "..")
}

fn overlapped(table: &Table) -> bool {
    let mut owners = BTreeSet::new();
    if table.owners.iter().any(|owner| !owners.insert(owner.name)) {
        return true;
    }
    let mut packages = BTreeSet::new();
    table
        .packages
        .iter()
        .any(|package| !packages.insert(package.name))
}

fn unowned(table: &Table) -> bool {
    let owners: BTreeSet<&str> = table.owners.iter().map(|owner| owner.name).collect();
    let packages: BTreeSet<&str> = table.packages.iter().map(|package| package.name).collect();
    table
        .packages
        .iter()
        .any(|package| !owners.contains(package.owner))
        || table
            .edges
            .iter()
            .any(|edge| !packages.contains(edge.from) || !packages.contains(edge.to))
}

fn split(edges: &[Edge]) -> bool {
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for edge in edges {
        if let Some(previous) = seen.insert(edge.to, edge.revision)
            && previous != edge.revision
        {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

fn cyclic(edges: &[Edge]) -> bool {
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    for edge in edges {
        outgoing.entry(edge.from).or_default().push(edge.to);
        nodes.insert(edge.from);
        nodes.insert(edge.to);
    }
    let mut color = BTreeMap::new();
    nodes
        .into_iter()
        .any(|node| walk(node, &outgoing, &mut color))
}

fn walk<'a>(
    node: &'a str,
    outgoing: &BTreeMap<&'a str, Vec<&'a str>>,
    color: &mut BTreeMap<&'a str, Color>,
) -> bool {
    match color.get(node).copied() {
        Some(Color::Gray) => return true,
        Some(Color::Black) => return false,
        None => {}
    }
    color.insert(node, Color::Gray);
    if let Some(children) = outgoing.get(node) {
        for child in children {
            if walk(child, outgoing, color) {
                return true;
            }
        }
    }
    color.insert(node, Color::Black);
    false
}

#[cfg(test)]
#[path = "manifest_admit_tests.rs"]
mod tests;
