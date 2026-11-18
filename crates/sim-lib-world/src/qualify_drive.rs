// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Qualify cases over the world composition identity.
//!
//! Each case calls the composition law in this crate. This module does not
//! open an executor of its own, does not admit a Linux provider, and does
//! not call a conformance pack.

use super::law::{self, Fault};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) fn composition() -> Result<Law, Fault> {
    let digest = law::revision()?;
    let report = law::qualify(&digest, &digest)?;
    let linux = report
        .providers
        .iter()
        .filter(|name| name.contains("linux"))
        .count();
    Ok(Law {
        detail: format!(
            "pages {} revision {} transitions {} stale-effects {} effects {} hot-delta {} samples {} providers {} stale-edges {} bound {} proof {} linux {}",
            report.first_page,
            report.retained_revision,
            report.transitions,
            report.stale_effects,
            report.effects,
            report.hot_delta,
            report.hot_samples.len(),
            report.providers.join(","),
            report.stale_edges,
            report.source_bound,
            report.unrelated_proof,
            linux,
        ),
        invokes: report.effects,
    })
}

pub(crate) fn linux() -> Result<Law, Fault> {
    match law::open_with_linux() {
        Err(Fault::LinuxProvider) => Ok(Law {
            detail: Fault::LinuxProvider.code().to_owned(),
            invokes: 0,
        }),
        Ok(()) => Err(Fault::Bound),
        Err(other) => Err(other),
    }
}

pub(crate) fn freeze() -> Result<Law, Fault> {
    let digest = law::revision()?;
    let wrong = "ab".repeat(32);
    let mismatch = match law::qualify(&digest, &wrong) {
        Err(fault) => fault,
        Ok(_) => return Err(Fault::Bound),
    };
    let empty = match law::qualify("", "") {
        Err(fault) => fault,
        Ok(_) => return Err(Fault::Bound),
    };
    Ok(Law {
        detail: format!("{} empty {}", mismatch.code(), empty.code()),
        invokes: 0,
    })
}

pub(crate) fn advice() -> Result<Law, Fault> {
    law::advice_rejected()?;
    Ok(Law {
        detail: Fault::AdviceRejected.code().to_owned(),
        invokes: 0,
    })
}

pub(crate) fn begin() -> Result<Law, Fault> {
    law::selection_is_distinct()?;
    law::hot_needs_begin()?;
    let (effects, stale, proof) = law::selection_without_begin()?;
    Ok(Law {
        detail: format!("distinct begin-required landed {effects} stale {stale} proof {proof}"),
        invokes: effects,
    })
}
