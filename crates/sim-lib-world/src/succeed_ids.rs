// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Semantic ids of the static construction batches.
//!
//! Each id is the observation id of the loaded snapshot for that batch
//! and selector. The tables do not change between calls.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use super::Stop;
use crate::fixture;
use crate::observe;

pub(super) fn matches_fixture(found: &str) -> Result<bool, Stop> {
    Ok(fixture_ids()?.contains(found))
}

fn fixture_ids() -> Result<&'static BTreeSet<String>, Stop> {
    static CACHE: OnceLock<BTreeSet<String>> = OnceLock::new();
    if let Some(ids) = CACHE.get() {
        return Ok(ids);
    }
    let mut ids = BTreeSet::new();
    for (batch, row) in fixture::observation_rows() {
        ids.insert(identity(batch, row.selector)?);
    }
    for (batch, selector) in observe::construction_snapshots() {
        ids.insert(identity(batch, selector)?);
    }
    Ok(CACHE.get_or_init(|| ids))
}

fn identity(batch: &str, selector: &str) -> Result<String, Stop> {
    let snapshot =
        observe::snapshot(batch, selector).map_err(|error| Stop::Broken(error.to_string()))?;
    super::semantic_id(&snapshot)
}
