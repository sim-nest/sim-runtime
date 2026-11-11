// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the compile record.

use super::{Capture, OwnedCompile, text_field};

pub(super) fn parsed_compile(capture: &Capture) -> Option<OwnedCompile> {
    Some(OwnedCompile {
        compile: text_field(&capture.value, "compile")?,
        seal: text_field(&capture.value, "seal")?,
        lock: text_field(&capture.value, "lock")?,
        missing: text_field(&capture.value, "missing")?,
        corrupt: text_field(&capture.value, "corrupt")?,
        target: text_field(&capture.value, "target")?,
        stub: text_field(&capture.value, "stub")?,
    })
}
