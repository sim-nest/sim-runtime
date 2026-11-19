// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded cold comparison for the portable phase union.
//!
//! The comparison runs the prove command. That cold run skips the compare
//! gate and keeps reuse disabled. False reuse stays zero only when the
//! loaded D1, D2, and D4 rows meet the frozen bounds. A scripted command
//! receipt is refused before launch and is not a member of the denominator.

use crate::WorldError;

/// Numbers observed by one loaded cold comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdStage {
    /// Phase gates in the public cold run.
    pub denominator: u64,
    /// False reuse summed from the loaded assay rows.
    pub false_reuse: u64,
    /// Unaffected basis points of D1.
    pub d1: u64,
    /// Unaffected basis points of D2.
    pub d2: u64,
    /// Unaffected basis points of D4.
    pub d4: u64,
    /// Whether that cold run kept reuse disabled.
    pub reuse_disabled: bool,
}

/// Runs the loaded cold comparison.
///
/// # Errors
/// Returns an error when the denominator is incomplete or the loaded records
/// miss the frozen bounds.
pub fn execute() -> Result<ColdStage, WorldError> {
    let observed = super::union::execute_cold_stage()?;
    Ok(ColdStage {
        denominator: observed.denominator,
        false_reuse: observed.false_reuse,
        d1: observed.d1,
        d2: observed.d2,
        d4: observed.d4,
        reuse_disabled: observed.reuse_disabled,
    })
}

/// Refuses one scripted command receipt before launch.
///
/// # Errors
/// Returns an error when that receipt is admitted.
pub fn scripted_receipt_is_refused(leaf: &str) -> Result<(), WorldError> {
    super::scripted_receipt_is_refused(leaf)
}

/// Counts a basis that disagrees with its affected set as false reuse.
///
/// # Errors
/// Returns an error when the disagreement is not counted.
pub fn contradictory_basis_is_false_reuse() -> Result<(), WorldError> {
    super::union::contradictory_basis_is_false_reuse()
}
