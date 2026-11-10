// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Runs one shipped `sim world` argument list through the loaded entrypoint.
///
/// # Errors
/// Returns the entrypoint refusal or evaluation error.
pub(crate) fn dispatch_argv(
    lib: &WorldCommandLib,
    argv: &[String],
) -> std::result::Result<Datum, String> {
    let entry = WorldEntrypoint {
        product: lib.product.clone(),
        load: lib.load.clone(),
    };
    match entry.dispatch(argv).map_err(|error| error.to_string())? {
        Outcome::Value(value) => Ok(value),
        Outcome::Refused { call, reason } => Err(format!("refused {call}: {reason}")),
        Outcome::OverCeiling { command, .. } => Err(format!("over ceiling: {command}")),
    }
}
