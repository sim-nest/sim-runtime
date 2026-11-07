// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sealed model readings and the desired values a plan compares them with.
//!
//! Desired values are not observation fields.

pub(super) struct Fact {
    pub(super) selector: &'static str,
    pub(super) state: &'static str,
    pub(super) value: Option<&'static str>,
}

pub(super) const FACTS: &[Fact] = &[
    Fact {
        selector: "git/branch/main",
        state: "current",
        value: Some("model-commit-main"),
    },
    Fact {
        selector: "git/branch/topic",
        state: "current",
        value: Some("model-commit-topic"),
    },
    Fact {
        selector: "git/tag/v1",
        state: "current",
        value: Some("model-tag-object-v1"),
    },
    Fact {
        selector: "git/direct/v1",
        state: "current",
        value: Some("model-tag-object-v1"),
    },
    Fact {
        selector: "git/peeled/v1",
        state: "current",
        value: Some("model-commit-peeled"),
    },
    Fact {
        selector: "registry/checksum/sim-kernel",
        state: "current",
        value: Some("model-checksum-kernel"),
    },
    Fact {
        selector: "ci/sim-kernel",
        state: "stale",
        value: Some("green"),
    },
    Fact {
        selector: "ci/sim-run",
        state: "partial",
        value: Some("pending"),
    },
    Fact {
        selector: "ci/sim-sdk",
        state: "uncertain",
        value: None,
    },
    Fact {
        selector: "mirror/sim-kernel",
        state: "lagging",
        value: Some("model-commit-topic"),
    },
    Fact {
        selector: "mirror/sim-private",
        state: "denied",
        value: None,
    },
];

pub(super) const DESIRED: &[(&str, &str)] = &[
    ("git/branch/main", "model-commit-main"),
    ("git/branch/topic", "model-commit-other"),
    ("git/tag/v1", "model-tag-object-v1"),
    ("git/direct/v1", "model-tag-object-v1"),
    ("git/peeled/v1", "model-commit-peeled"),
    ("registry/checksum/sim-kernel", "model-checksum-kernel"),
    ("ci/sim-kernel", "green"),
    ("ci/sim-run", "pending"),
    ("ci/sim-sdk", "green"),
    ("mirror/sim-kernel", "model-commit-topic"),
    ("mirror/sim-private", "model-commit-main"),
];
