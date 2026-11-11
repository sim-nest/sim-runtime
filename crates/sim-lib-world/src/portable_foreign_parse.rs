// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the foreign succeed records.

use super::{Capture, OwnedForeign, outcome, project_of};

pub(super) fn parsed_foreign(capture: &Capture) -> Option<OwnedForeign> {
    if project_of(&capture.specimen).as_deref() != Some("lantern") {
        return None;
    }
    if project_of(&capture.second).as_deref() != Some("harbor") {
        return None;
    }
    Some(OwnedForeign {
        specimen: outcome(&capture.specimen)?,
        second: outcome(&capture.second)?,
        branch: outcome(&capture.branch)?,
    })
}
