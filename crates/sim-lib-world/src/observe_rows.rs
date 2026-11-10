// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted observation documents for the loaded command.
//!
//! A document starts with `sim-world-rows-v1`. A `label` line names the batch
//! field. An `envelope` line names the diagnostic envelope and stays outside
//! semantic identity. `realization native` refuses without reading a port.
//! Each later line is `port selector status value`. A value of `-` is absent.
//! Text that is not this document is an unknown batch.

use sim_kernel::Datum;

use super::{Description, Semantic, describe_row, diff_datum, gap, snapshot_datum};
use crate::WorldError;

struct RowDocument {
    label: String,
    envelope: String,
    native: bool,
    rows: Vec<OwnedRow>,
}

struct OwnedRow {
    port: String,
    selector: String,
    status: String,
    value: Option<String>,
}

/// Reads one selector from an admitted observation document.
///
/// # Errors
/// Returns an error when a row breaks the status and value binding.
pub(crate) fn snapshot_document(text: &str, selector: &str) -> Result<Datum, WorldError> {
    let Some(document) = parse_row_document(text) else {
        return Ok(gap("unknown-batch", text, selector));
    };
    if document.native && selector.is_empty() {
        return Ok(gap("empty-selector", &document.label, selector));
    }
    if document.native {
        let described = super::native_description(selector)?;
        return Ok(snapshot_datum(&document.label, selector, &described));
    }
    read_rows(&document, selector)
}

/// Compares one selector across two admitted observation documents.
///
/// # Errors
/// Returns an error when a row breaks the status and value binding.
pub(crate) fn diff_documents(
    before_text: &str,
    after_text: &str,
    selector: &str,
) -> Result<Datum, WorldError> {
    let Some(before) = parse_row_document(before_text) else {
        return Ok(gap("unknown-batch", before_text, selector));
    };
    let Some(after) = parse_row_document(after_text) else {
        return Ok(gap("unknown-batch", after_text, selector));
    };
    if selector.is_empty() {
        return Ok(gap("empty-selector", &before.label, selector));
    }
    if capability_missing(&before, selector) || capability_missing(&after, selector) {
        return Ok(gap("missing-capability", &before.label, selector));
    }
    let left = describe_side(&before, selector)?;
    let right = describe_side(&after, selector)?;
    if left.semantic == right.semantic {
        return Ok(diff_datum(
            &before.label,
            &after.label,
            selector,
            None,
            None,
        ));
    }
    Ok(diff_datum(
        &before.label,
        &after.label,
        selector,
        Some(&left),
        Some(&right),
    ))
}

fn parse_row_document(text: &str) -> Option<RowDocument> {
    let mut lines = text.lines();
    if lines.next()? != "sim-world-rows-v1" {
        return None;
    }
    let mut label = "admitted".to_owned();
    let mut envelope = "model-log".to_owned();
    let mut native = false;
    let mut rows = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(' ');
        let head = parts.next()?;
        if head == "label" {
            label = single_token(&mut parts)?.to_owned();
            continue;
        }
        if head == "envelope" {
            envelope = single_token(&mut parts)?.to_owned();
            continue;
        }
        if head == "realization" {
            if single_token(&mut parts)? != "native" {
                return None;
            }
            native = true;
            continue;
        }
        let selector = parts.next()?.to_owned();
        let status = parts.next()?.to_owned();
        let value = parts.next()?.to_owned();
        if parts.next().is_some() {
            return None;
        }
        rows.push(OwnedRow {
            port: head.to_owned(),
            selector,
            status,
            value: (value != "-").then_some(value),
        });
    }
    Some(RowDocument {
        label,
        envelope,
        native,
        rows,
    })
}

fn single_token<'a>(parts: &mut std::str::Split<'a, char>) -> Option<&'a str> {
    let token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(token)
}

fn capability_missing(document: &RowDocument, selector: &str) -> bool {
    if document.native {
        return false;
    }
    !port_declared(document, selector)
}

fn describe_side(document: &RowDocument, selector: &str) -> Result<Description, WorldError> {
    if document.native {
        return super::native_description(selector);
    }
    describe_owned(document, selector)
}

fn port_declared(document: &RowDocument, selector: &str) -> bool {
    let Some((prefix, _)) = selector.split_once('/') else {
        return false;
    };
    document.rows.iter().any(|row| row.port == prefix)
}

fn describe_owned(document: &RowDocument, selector: &str) -> Result<Description, WorldError> {
    let found = document
        .rows
        .iter()
        .find(|row| row.selector == selector)
        .map(|row| Semantic {
            port: row.port.as_str(),
            selector: row.selector.as_str(),
            status: row.status.as_str(),
            value: row.value.as_deref(),
        });
    let declared = selector.split_once('/').map(|(prefix, _)| prefix);
    describe_row(found, selector, &document.envelope, declared)
}

fn read_rows(document: &RowDocument, selector: &str) -> Result<Datum, WorldError> {
    if selector.is_empty() {
        return Ok(gap("empty-selector", &document.label, selector));
    }
    if !port_declared(document, selector) {
        return Ok(gap("missing-capability", &document.label, selector));
    }
    let described = describe_owned(document, selector)?;
    Ok(snapshot_datum(&document.label, selector, &described))
}
