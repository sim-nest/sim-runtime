// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::ZERO_SEAL;
use super::parse::MARKER;

#[test]
fn the_loaded_work_command_reports_the_packet_law() {
    let loaded = super::execute().expect("work");
    assert_eq!(loaded.admitted, "admitted");
    assert_eq!(loaded.oversize, "oversize");
    assert_eq!(loaded.incomplete, "incomplete");
    assert_eq!(loaded.malformed, "malformed-bound");
    assert_eq!(loaded.descent, "no-descent");
    assert_eq!(loaded.namespace, "namespace");
    assert_eq!(loaded.role, "role");
    assert_eq!(loaded.stale, "stale-copy");
    assert_eq!(loaded.schema, "schema");
    assert_eq!(loaded.effects, "effects-or-approval");
    assert_eq!(loaded.leakage, "private-leakage");
    let seals = [
        loaded.admitted_seal.as_str(),
        loaded.oversize_seal.as_str(),
        loaded.incomplete_seal.as_str(),
        loaded.malformed_seal.as_str(),
        loaded.descent_seal.as_str(),
        loaded.namespace_seal.as_str(),
        loaded.role_seal.as_str(),
        loaded.stale_seal.as_str(),
        loaded.schema_seal.as_str(),
        loaded.effects_seal.as_str(),
        loaded.leakage_seal.as_str(),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for seal in seals {
        assert_ne!(seal, ZERO_SEAL);
        assert!(seen.insert(seal), "{seal}");
    }
    let again = super::execute().expect("stable");
    assert_eq!(loaded, again);
    let rendered = format!("{loaded:?}");
    assert!(!rendered.contains("completion/claim-"));
    assert!(!rendered.contains(MARKER));
    assert!(!rendered.contains("/home/"));
    assert!(!rendered.contains("bo-private"));
}
