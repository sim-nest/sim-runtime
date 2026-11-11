// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Model delivery canary over one validated source head.
//!
//! A fresh model branch is chosen under a separate purpose grant. The
//! delivery runs once, recovery observes first, and the repeated
//! reconciliation does no further work. A wrong or consumed purpose grant
//! refuses. No public branch, tag, package, or merge is written.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::deliver::private_input;
use crate::product::DISCLOSURE_FACT;
use crate::query;
use crate::service;

#[path = "canary_memory.rs"]
pub(crate) mod memory;
#[path = "canary_run.rs"]
mod run;

const CANARY_PURPOSE: &str = "world/model-canary";

/// A one-shot purpose grant, separate from the delivery capability.
pub(crate) struct PurposeGrant {
    purpose: String,
    consumed: bool,
}

impl PurposeGrant {
    /// Issues a grant for the caller-supplied purpose.
    pub(crate) fn issue(purpose: &str) -> Self {
        Self {
            purpose: purpose.to_owned(),
            consumed: false,
        }
    }

    /// Marks this grant spent. A later admission refuses.
    pub(crate) fn consume(&mut self) {
        self.consumed = true;
    }

    fn admit(&self) -> Result<(), &'static str> {
        if self.purpose != CANARY_PURPOSE {
            return Err("wrong-purpose");
        }
        if self.consumed {
            return Err("consumed-purpose");
        }
        Ok(())
    }
}

/// Runs one canary from a fresh or already spent purpose grant.
///
/// # Errors
/// Returns an error when the lifecycle breaks its contract.
pub(crate) fn run(
    realization: &str,
    source: &str,
    destination: &str,
    purpose: &str,
    presentation: &str,
) -> Result<Datum, WorldError> {
    if private_input(
        realization,
        source,
        destination,
        purpose,
        Some(presentation),
    ) {
        return Ok(gap(
            realization,
            source,
            destination,
            purpose,
            "private-leakage",
        ));
    }
    let mut grant = PurposeGrant::issue(purpose);
    if presentation == "spent" {
        grant.consume();
    } else if presentation != "issued" {
        return Ok(gap(
            realization,
            source,
            destination,
            purpose,
            "unknown-presentation",
        ));
    }
    run_grant(realization, source, destination, &mut grant)
}

/// Runs one canary with the caller-held purpose grant.
///
/// A successful delivery consumes the grant. A second call refuses.
///
/// # Errors
/// Returns an error when the lifecycle breaks its contract.
pub(crate) fn run_grant(
    realization: &str,
    source: &str,
    destination: &str,
    grant: &mut PurposeGrant,
) -> Result<Datum, WorldError> {
    if private_input(
        realization,
        source,
        destination,
        &grant.purpose,
        Some(source),
    ) {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            "private-leakage",
        ));
    }
    if source.is_empty() {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            "empty-source",
        ));
    }
    if destination.is_empty() {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            "empty-destination",
        ));
    }
    let Some(head) = validated_source(realization, source)? else {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            source_reason(realization, source)?,
        ));
    };
    apply_canary(realization, source, destination, grant, &head)
}

fn apply_canary(
    realization: &str,
    source: &str,
    destination: &str,
    grant: &mut PurposeGrant,
    head: &Head,
) -> Result<Datum, WorldError> {
    if private_input(
        realization,
        source,
        destination,
        &grant.purpose,
        Some(&head.value),
    ) {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            "private-leakage",
        ));
    }
    if let Some(reason) = blocked_destination(realization, source, destination)? {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            reason,
        ));
    }
    if let Err(reason) = grant.admit() {
        return Ok(gap(
            realization,
            source,
            destination,
            &grant.purpose,
            reason,
        ));
    }
    let claim = match memory::claim(memory::Input {
        realization,
        source,
        destination,
        purpose: &grant.purpose,
        kind: &head.kind,
        provenance: &head.provenance,
        state: &head.state,
        value: &head.value,
    }) {
        memory::Reservation::Ready(found) => {
            grant.consume();
            return Ok(found);
        }
        memory::Reservation::Mine(claim) => claim,
    };
    let reconciliation = run::reconcile(realization, destination, &head.value)?;
    grant.consume();
    let delivered = record(realization, source, destination, head, &reconciliation);
    claim.commit(&delivered);
    Ok(delivered)
}

struct Head {
    kind: String,
    provenance: String,
    state: String,
    value: String,
}

fn validated_source(realization: &str, source: &str) -> Result<Option<Head>, WorldError> {
    let snapshot = service::snapshot(realization, source)?;
    let Some(row) = observation(&snapshot)? else {
        return Ok(None);
    };
    if row.kind == "branch"
        && row.state == "current"
        && let Some(value) = row.value.clone()
    {
        return Ok(Some(Head {
            kind: row.kind,
            provenance: row.provenance,
            state: row.state,
            value,
        }));
    }
    Ok(None)
}

fn source_reason(realization: &str, source: &str) -> Result<&'static str, WorldError> {
    let snapshot = service::snapshot(realization, source)?;
    if let Some(reason) = gap_reason(&snapshot)? {
        return Ok(reason);
    }
    let Some(row) = observation(&snapshot)? else {
        return Ok("source-not-validated");
    };
    if row.state == "unavailable" {
        return Ok("unavailable-receiver");
    }
    Ok("source-not-validated")
}

fn blocked_destination(
    realization: &str,
    source: &str,
    destination: &str,
) -> Result<Option<&'static str>, WorldError> {
    if destination.split('/').any(|part| part == "merge") {
        return Ok(Some("public-target"));
    }
    let snapshot = service::snapshot(realization, destination)?;
    if gap_reason(&snapshot)?.is_some() {
        return Ok(Some("destination-not-branch"));
    }
    let Some(row) = observation(&snapshot)? else {
        return Ok(Some("destination-not-branch"));
    };
    let public_kind = matches!(row.kind.as_str(), "tag" | "checksum" | "direct" | "peeled");
    if destination == source || row.state != "absent" || public_kind {
        return Ok(Some("public-target"));
    }
    if row.kind == "branch" {
        return Ok(None);
    }
    Ok(Some("destination-not-branch"))
}

struct Observation {
    kind: String,
    provenance: String,
    state: String,
    value: Option<String>,
}

fn observation(snapshot: &Datum) -> Result<Option<Observation>, WorldError> {
    match snapshot {
        Datum::Node { tag, .. } if tag.as_qualified_str() == "world/service-gap-v1" => Ok(None),
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/service-snapshot-v1" => {
            let rows = vector_field(fields, "observations")?;
            let row = node_fields(
                rows.first().ok_or_else(|| {
                    WorldError::Qualification("service snapshot has no observation".to_owned())
                })?,
                "world/service-observation-v1",
            )?;
            Ok(Some(Observation {
                kind: required(row, "kind")?,
                provenance: required(row, "provenance")?,
                state: required(row, "state")?,
                value: optional(row, "value")?,
            }))
        }
        _ => Err(WorldError::Qualification(
            "service snapshot is not readable".to_owned(),
        )),
    }
}

fn gap_reason(snapshot: &Datum) -> Result<Option<&'static str>, WorldError> {
    match snapshot {
        Datum::Node { tag, fields } if tag.as_qualified_str() == "world/service-gap-v1" => {
            let reason = required(fields, "reason")?;
            Ok(Some(match reason.as_str() {
                "empty-selector" => "empty-source",
                "unknown-realization" => "unknown-realization",
                _ => "source-not-validated",
            }))
        }
        _ => Ok(None),
    }
}

fn record(
    realization: &str,
    source: &str,
    destination: &str,
    head: &Head,
    reconciliation: &run::Reconciliation,
) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "canary-v1"),
        fields: vec![
            (Symbol::new("class"), string(reconciliation.class)),
            (
                Symbol::new("destination"),
                branch_object(destination, &head.value),
            ),
            (Symbol::new("disclosure"), string(DISCLOSURE_FACT)),
            (
                Symbol::new("dispatches"),
                query::count_datum(reconciliation.dispatches),
            ),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("grant-purpose"), string(CANARY_PURPOSE)),
            (Symbol::new("grant-state"), string("consumed")),
            (
                Symbol::new("observations"),
                query::count_datum(reconciliation.observations),
            ),
            (
                Symbol::new("performer-calls"),
                query::count_datum(reconciliation.performer_calls),
            ),
            (Symbol::new("realization"), string(realization)),
            (Symbol::new("recovered"), Datum::Bool(true)),
            (
                Symbol::new("repeat-dispatches"),
                query::count_datum(reconciliation.repeat_dispatches),
            ),
            (
                Symbol::new("repeat-performer-calls"),
                query::count_datum(reconciliation.repeat_performer_calls),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("source"), source_object(source, head)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn source_object(selector: &str, head: &Head) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "canary-source-v1"),
        fields: vec![
            (Symbol::new("kind"), string(&head.kind)),
            (Symbol::new("provenance"), string(&head.provenance)),
            (Symbol::new("selector"), string(selector)),
            (Symbol::new("state"), string(&head.state)),
            (Symbol::new("value"), string(&head.value)),
        ],
    }
}

fn branch_object(selector: &str, value: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "canary-branch-v1"),
        fields: vec![
            (
                Symbol::new("provenance"),
                string(&format!("model/{selector}")),
            ),
            (Symbol::new("selector"), string(selector)),
            (Symbol::new("state"), string("current")),
            (Symbol::new("value"), string(value)),
        ],
    }
}

fn gap(realization: &str, source: &str, destination: &str, purpose: &str, reason: &str) -> Datum {
    let leaked = private_input(realization, source, destination, purpose, Some(reason));
    let reason = if leaked { "private-leakage" } else { reason };
    Datum::Node {
        tag: Symbol::qualified("world", "canary-gap-v1"),
        fields: vec![
            (Symbol::new("destination"), string(&blank(destination))),
            (Symbol::new("dispatches"), query::count_datum(0)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("purpose"), string(&blank(purpose))),
            (Symbol::new("realization"), string(&blank(realization))),
            (Symbol::new("reason"), string(reason)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("source"), string(&blank(source))),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn blank(value: &str) -> String {
    if private_input("", value, "", "", None) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn string(value: &str) -> Datum {
    Datum::String(value.to_owned())
}

fn node_fields<'a>(value: &'a Datum, tag: &str) -> Result<&'a [(Symbol, Datum)], WorldError> {
    match value {
        Datum::Node { tag: found, fields } if found.as_qualified_str() == tag => Ok(fields),
        _ => Err(WorldError::Qualification(format!("missing {tag}"))),
    }
}

fn vector_field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a [Datum], WorldError> {
    match field(fields, name)? {
        Datum::Vector(rows) => Ok(rows),
        _ => Err(WorldError::Qualification(format!("{name} is not a vector"))),
    }
}

fn required(fields: &[(Symbol, Datum)], name: &str) -> Result<String, WorldError> {
    match field(fields, name)? {
        Datum::String(value) => Ok(value.clone()),
        _ => Err(WorldError::Qualification(format!("{name} is not text"))),
    }
}

fn optional(fields: &[(Symbol, Datum)], name: &str) -> Result<Option<String>, WorldError> {
    match fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
    {
        Some(Datum::String(value)) => Ok(Some(value.clone())),
        None => Ok(None),
        _ => Err(WorldError::Qualification(format!("{name} is not text"))),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a Datum, WorldError> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .ok_or_else(|| WorldError::Qualification(format!("missing {name}")))
}

/// Delivers one already observed source head under the purpose grant.
#[cfg(test)]
pub(crate) fn deliver_observed_head(
    realization: &str,
    source: &str,
    destination: &str,
    grant: &mut PurposeGrant,
    value: &str,
) -> Result<Datum, WorldError> {
    apply_canary(
        realization,
        source,
        destination,
        grant,
        &Head {
            kind: "branch".to_owned(),
            provenance: format!("model/{source}"),
            state: "current".to_owned(),
            value: value.to_owned(),
        },
    )
}
