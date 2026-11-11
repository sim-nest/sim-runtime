// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the evidence record.

use super::{Capture, OwnedEvidence, text_field};

pub(super) fn parsed_evidence(capture: &Capture) -> Option<OwnedEvidence> {
    Some(OwnedEvidence {
        key: text_field(&capture.value, "key")?,
        receipt: text_field(&capture.value, "receipt")?,
        telemetry: text_field(&capture.value, "telemetry")?,
        domain: text_field(&capture.value, "domain")?,
        revoked: text_field(&capture.value, "revoked")?,
        missing: text_field(&capture.value, "missing")?,
        storage: text_field(&capture.value, "storage")?,
    })
}
