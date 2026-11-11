// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Remembered delivery qualifications.
//!
//! One reading opens one journal. A later call with that same reading returns
//! the qualification instead of opening another journal. A different reading
//! qualifies again.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::super::Reading;
use super::Qualified;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    realization: String,
    selector: String,
    desired: String,
    reason: String,
    kind: String,
    state: String,
    value: Option<String>,
    provenance: String,
}

static REMEMBERED: LazyLock<Mutex<HashMap<Key, Qualified>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn recall(
    realization: &str,
    selector: &str,
    desired: &str,
    reason: &str,
    reading: &Reading,
) -> Option<Qualified> {
    let key = key(realization, selector, desired, reason, reading);
    remembered().get(&key).cloned()
}

pub(super) fn store(
    realization: &str,
    selector: &str,
    desired: &str,
    reason: &str,
    reading: &Reading,
    qualified: &Qualified,
) {
    let key = key(realization, selector, desired, reason, reading);
    remembered().insert(key, qualified.clone());
}

fn key(realization: &str, selector: &str, desired: &str, reason: &str, reading: &Reading) -> Key {
    Key {
        realization: realization.to_owned(),
        selector: selector.to_owned(),
        desired: desired.to_owned(),
        reason: reason.to_owned(),
        kind: reading.kind.clone(),
        state: reading.state.clone(),
        value: reading.value.clone(),
        provenance: reading.provenance.clone(),
    }
}

fn remembered() -> std::sync::MutexGuard<'static, HashMap<Key, Qualified>> {
    REMEMBERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
