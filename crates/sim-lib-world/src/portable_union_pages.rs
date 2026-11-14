// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Plan pages for the phase-union denominator.
//!
//! A call with no view lists the live gates and does not run a leaf.
//! A phase-union view is admitted before that page is returned. Admission
//! reruns each grounded leaf. A missing or stale view is a refresh gap.
//! A continuation token resumes the next page. The frontier token opens
//! the frontier of that same view.

use sim_kernel::Datum;

use crate::WorldError;

use super::super::record::{self, gap};

pub(crate) fn plan(token: Option<&str>, view: Option<&str>) -> Result<Datum, WorldError> {
    if is_union(view) {
        if let Some(token) = token
            && !super::super::is_token(token)
        {
            return Ok(gap("plan", "stale-or-missing"));
        }
        if super::admitted(view)?.is_none() {
            return Ok(gap("plan", "refresh-needed"));
        }
    }
    page(token)
}

pub(crate) fn resume(token: &str, view: Option<&str>) -> Result<Datum, WorldError> {
    if !super::super::is_token(token) {
        return Ok(gap("resume", "stale-or-missing"));
    }
    let leaves = super::phase_names()?;
    let subject = super::subject()?;
    let pages = super::super::page_count(leaves.len());
    if let Some(page) = super::super::page_for(token, &subject, pages)? {
        if is_union(view) && super::admitted(view)?.is_none() {
            return Ok(gap("resume", "refresh-needed"));
        }
        return record::plan_record(&leaves, &subject, page, pages);
    }
    if token == super::super::frontier_token(&subject)? {
        return super::frontier(view);
    }
    if is_union(view) && super::admitted(view)?.is_none() {
        return Ok(gap("resume", "refresh-needed"));
    }
    Ok(gap("resume", "stale-or-missing"))
}

fn page(token: Option<&str>) -> Result<Datum, WorldError> {
    let leaves = super::phase_names()?;
    let subject = super::subject()?;
    let pages = super::super::page_count(leaves.len());
    let page = match token {
        None => 0,
        Some(token) => match super::super::page_for(token, &subject, pages)? {
            Some(page) => page,
            None => return Ok(gap("plan", "stale-or-missing")),
        },
    };
    record::plan_record(&leaves, &subject, page, pages)
}

fn is_union(view: Option<&str>) -> bool {
    view.is_some_and(|text| text.starts_with(super::HEADER))
}
