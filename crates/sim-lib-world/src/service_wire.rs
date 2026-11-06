// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Byte encoding of one service observation.
//!
//! Provenance is a required field. Decoding rejects a truncated or trailing
//! encoding, so a desired value cannot be appended to a reading.

use sim_kernel::{Datum, Symbol};

use super::{Reading, row_datum};
use crate::WorldError;

const WIRE_MAGIC: &[u8] = b"service-observation-v1\n";

/// Encodes one service observation, including its provenance.
///
/// # Errors
/// Returns an error when the snapshot has no observation row.
pub(crate) fn encode_observation(snapshot: &Datum) -> Result<Vec<u8>, WorldError> {
    let row = observation_fields(snapshot)?;
    let mut out = Vec::from(WIRE_MAGIC);
    write_text(&mut out, required(&row, "kind")?)?;
    write_text(&mut out, required(&row, "policy")?)?;
    write_text(&mut out, required(&row, "port")?)?;
    write_text(&mut out, required(&row, "provenance")?)?;
    write_text(&mut out, required(&row, "selector")?)?;
    write_text(&mut out, required(&row, "state")?)?;
    match optional(&row, "value")? {
        Some(value) => {
            out.push(1);
            write_text(&mut out, value)?;
        }
        None => out.push(0),
    }
    Ok(out)
}

/// Encodes the snapshot row and rejects a decode that is not that same
/// observation.
///
/// # Errors
/// Returns an error when the snapshot has no observation row or the decoded
/// row differs from it.
pub(super) fn preserve(snapshot: &Datum) -> Result<(), WorldError> {
    let decoded = decode_observation(&encode_observation(snapshot)?)?;
    let original = Datum::Node {
        tag: Symbol::qualified("world", "service-observation-v1"),
        fields: observation_fields(snapshot)?,
    };
    if decoded != original {
        return Err(WorldError::Qualification(
            "service observation did not preserve its fields".to_owned(),
        ));
    }
    Ok(())
}

/// Decodes one service observation encoded by [`encode_observation`].
///
/// # Errors
/// Returns an error when the bytes are truncated, trailing, or not the
/// observation encoding.
pub(crate) fn decode_observation(bytes: &[u8]) -> Result<Datum, WorldError> {
    let Some(rest) = bytes.strip_prefix(WIRE_MAGIC) else {
        return Err(WorldError::Qualification(
            "service observation encoding is not recognized".to_owned(),
        ));
    };
    let mut rest = rest;
    let reading = Reading {
        kind: read_text(&mut rest)?,
        policy: policy_of(&read_text(&mut rest)?)?,
        port: read_text(&mut rest)?,
        provenance: read_text(&mut rest)?,
        selector: read_text(&mut rest)?,
        state: read_text(&mut rest)?,
        value: read_value(&mut rest)?,
        called: false,
    };
    if !rest.is_empty() {
        return Err(WorldError::Qualification(
            "service observation encoding has trailing bytes".to_owned(),
        ));
    }
    Ok(row_datum(&reading))
}

fn observation_fields(snapshot: &Datum) -> Result<Vec<(Symbol, Datum)>, WorldError> {
    let fields = node_fields(snapshot, "world/service-snapshot-v1")?;
    let observations = vector_field(&fields, "observations")?;
    let Some(row) = observations.first() else {
        return Err(WorldError::Qualification(
            "service snapshot has no observation".to_owned(),
        ));
    };
    node_fields(row, "world/service-observation-v1")
}

fn node_fields(value: &Datum, tag: &str) -> Result<Vec<(Symbol, Datum)>, WorldError> {
    match value {
        Datum::Node {
            tag: actual,
            fields,
        } if actual.as_qualified_str() == tag => Ok(fields.clone()),
        _ => Err(WorldError::Qualification(format!(
            "expected {tag} while encoding a service observation"
        ))),
    }
}

fn vector_field(fields: &[(Symbol, Datum)], name: &str) -> Result<Vec<Datum>, WorldError> {
    match field_datum(fields, name)? {
        Datum::Vector(items) => Ok(items),
        _ => Err(WorldError::Qualification(format!(
            "service field {name} is not a vector"
        ))),
    }
}

fn required(fields: &[(Symbol, Datum)], name: &str) -> Result<String, WorldError> {
    match field_datum(fields, name)? {
        Datum::String(value) => Ok(value),
        _ => Err(WorldError::Qualification(format!(
            "service field {name} is not text"
        ))),
    }
}

fn optional(fields: &[(Symbol, Datum)], name: &str) -> Result<Option<String>, WorldError> {
    match fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
    {
        Some(Datum::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(WorldError::Qualification(format!(
            "service field {name} is not text"
        ))),
        None => Ok(None),
    }
}

fn field_datum(fields: &[(Symbol, Datum)], name: &str) -> Result<Datum, WorldError> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| WorldError::Qualification(format!("service field {name} is missing")))
}

fn write_text(out: &mut Vec<u8>, value: String) -> Result<(), WorldError> {
    let len = u32::try_from(value.len()).map_err(|_| {
        WorldError::Qualification("service observation field is too long".to_owned())
    })?;
    out.extend(len.to_be_bytes());
    out.extend(value.as_bytes());
    Ok(())
}

fn read_text(input: &mut &[u8]) -> Result<String, WorldError> {
    if input.len() < 4 {
        return Err(WorldError::Qualification(
            "service observation encoding is truncated".to_owned(),
        ));
    }
    let len = usize::try_from(u32::from_be_bytes([input[0], input[1], input[2], input[3]]))
        .map_err(|_| {
            WorldError::Qualification("service observation encoding is truncated".to_owned())
        })?;
    *input = &input[4..];
    if input.len() < len {
        return Err(WorldError::Qualification(
            "service observation encoding is truncated".to_owned(),
        ));
    }
    let text = std::str::from_utf8(&input[..len]).map_err(|_| {
        WorldError::Qualification("service observation encoding is not text".to_owned())
    })?;
    *input = &input[len..];
    Ok(text.to_owned())
}

fn read_value(input: &mut &[u8]) -> Result<Option<String>, WorldError> {
    let Some((flag, rest)) = input.split_first() else {
        return Err(WorldError::Qualification(
            "service observation encoding is truncated".to_owned(),
        ));
    };
    *input = rest;
    match flag {
        0 => Ok(None),
        1 => Ok(Some(read_text(input)?)),
        _ => Err(WorldError::Qualification(
            "service observation encoding has a bad value flag".to_owned(),
        )),
    }
}

fn policy_of(value: &str) -> Result<&'static str, WorldError> {
    match value {
        super::POLICY_MODEL => Ok(super::POLICY_MODEL),
        super::POLICY_NATIVE => Ok(super::POLICY_NATIVE),
        super::POLICY_REAL => Ok(super::POLICY_REAL),
        _ => Err(WorldError::Qualification(
            "service observation encoding has an unknown policy".to_owned(),
        )),
    }
}
