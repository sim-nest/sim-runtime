// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published composition specimen.
//!
//! Each case calls the published consumer-seams functions. This module does
//! not open an executor of its own and does not admit a Linux provider.

use sim_conformance_packs::compiled_executor_closure;
use sim_conformance_packs::packs::portability::consumer_seams::{
    SeamFault, advice_rejected, hot_needs_begin, open_with_linux, qualify, selection_is_distinct,
    selection_without_begin,
};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) fn composition() -> Result<Law, SeamFault> {
    let digest = compiled_executor_closure();
    let report = qualify(&digest, &digest)?;
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

pub(crate) fn linux() -> Result<Law, SeamFault> {
    match open_with_linux() {
        Err(SeamFault::LinuxProvider) => Ok(Law {
            detail: "linux-provider".to_owned(),
            invokes: 0,
        }),
        Ok(()) => Err(SeamFault::Bound),
        Err(other) => Err(other),
    }
}

pub(crate) fn freeze() -> Result<Law, SeamFault> {
    let digest = compiled_executor_closure();
    let wrong = "ab".repeat(32);
    match qualify(&digest, &wrong) {
        Err(SeamFault::FreezeMismatch) => {}
        Ok(_) => return Err(SeamFault::Bound),
        Err(other) => return Err(other),
    }
    match qualify("", "") {
        Err(SeamFault::FreezeMismatch) => Ok(Law {
            detail: "freeze-mismatch empty freeze-mismatch".to_owned(),
            invokes: 0,
        }),
        Ok(_) => Err(SeamFault::Bound),
        Err(other) => Err(other),
    }
}

pub(crate) fn advice() -> Result<Law, SeamFault> {
    advice_rejected()?;
    Ok(Law {
        detail: "advice-rejected".to_owned(),
        invokes: 0,
    })
}

pub(crate) fn begin() -> Result<Law, SeamFault> {
    selection_is_distinct()?;
    hot_needs_begin()?;
    let (effects, stale, proof) = selection_without_begin()?;
    Ok(Law {
        detail: format!("distinct begin-required landed {effects} stale {stale} proof {proof}"),
        invokes: effects,
    })
}
