// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{
    EDGES, Edge, LINUX_OWNERS, OWNER, OWNERS, Owner, PACKAGES, PIN_ALPHA, Package, Table,
    Visibility, admit, table,
};

const NESTED_ESCAPE: &[Edge] = &[Edge {
    from: "codec",
    to: "core",
    revision: PIN_ALPHA,
    path: "crates/../../etc",
}];

const SELF_LOOP: &[Edge] = &[Edge {
    from: "core",
    to: "core",
    revision: PIN_ALPHA,
    path: "crates/core",
}];

const CHAIN: &[Edge] = &[
    Edge {
        from: "note",
        to: "codec",
        revision: PIN_ALPHA,
        path: "crates/codec",
    },
    Edge {
        from: "codec",
        to: "core",
        revision: PIN_ALPHA,
        path: "crates/core",
    },
];

const LONE_OWNERS: &[Owner] = &[Owner {
    name: "linux-tools",
    visibility: Visibility::Public,
    pin: PIN_ALPHA,
    observed: PIN_ALPHA,
    linux: false,
}];

const LONE_PACKAGES: &[Package] = &[Package {
    name: "core",
    owner: "linux-tools",
    source: "crates/core",
}];

fn census(table: &Table) -> super::Census {
    admit(table).expect("admitted")
}

#[test]
fn the_walk_counts_a_coherent_table_and_refuses_each_defect() {
    let owner = census(&OWNER);
    assert_eq!(owner.owners, OWNER.owners.len() as u64);
    assert_eq!(owner.packages, OWNER.packages.len() as u64);
    assert_eq!(owner.edges, OWNER.edges.len() as u64);
    let chain = Table {
        owners: OWNERS,
        packages: PACKAGES,
        edges: CHAIN,
    };
    let chained = census(&chain);
    assert_eq!(chained.edges, 2);
    let lone = Table {
        owners: LONE_OWNERS,
        packages: LONE_PACKAGES,
        edges: &[],
    };
    let alone = census(&lone);
    assert_eq!(alone.owners, 1);
    assert_eq!(alone.edges, 0);
    assert_eq!(
        admit(&Table {
            owners: LINUX_OWNERS,
            packages: PACKAGES,
            edges: EDGES,
        })
        .expect_err("linux")
        .code(),
        "linux-target"
    );
    assert_eq!(
        admit(&table("missing-pin").expect("table"))
            .expect_err("pin")
            .code(),
        "missing-pin"
    );
    assert_eq!(
        admit(&table("pin-drift").expect("table"))
            .expect_err("drift")
            .code(),
        "pin-drift"
    );
    assert_eq!(
        admit(&table("private-pin").expect("table"))
            .expect_err("private")
            .code(),
        "private-pin"
    );
    assert_eq!(
        admit(&table("escape").expect("table"))
            .expect_err("escape")
            .code(),
        "escape"
    );
    assert_eq!(
        admit(&Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: NESTED_ESCAPE,
        })
        .expect_err("nested")
        .code(),
        "escape"
    );
    assert_eq!(
        admit(&table("fabricated").expect("table"))
            .expect_err("source")
            .code(),
        "fabricated"
    );
    assert_eq!(
        admit(&table("overlap").expect("table"))
            .expect_err("overlap")
            .code(),
        "overlap"
    );
    assert_eq!(
        admit(&table("missing-owner").expect("table"))
            .expect_err("owner")
            .code(),
        "missing-owner"
    );
    assert_eq!(
        admit(&Table {
            owners: &[],
            packages: PACKAGES,
            edges: EDGES,
        })
        .expect_err("empty")
        .code(),
        "missing-owner"
    );
    assert_eq!(
        admit(&table("split-source").expect("table"))
            .expect_err("split")
            .code(),
        "split-source"
    );
    assert_eq!(
        admit(&table("cycle").expect("table"))
            .expect_err("cycle")
            .code(),
        "cycle"
    );
    assert_eq!(
        admit(&Table {
            owners: OWNERS,
            packages: PACKAGES,
            edges: SELF_LOOP,
        })
        .expect_err("self")
        .code(),
        "cycle"
    );
}
