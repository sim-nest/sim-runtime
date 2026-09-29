// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Strict reconstruction of the existing boot-installed command specification.

use super::request_wire::{content_id, field, fields, string};
use super::*;
use crate::{BindingValue, PrivateArtifactRef};

#[path = "command_decode/sandbox.rs"]
mod sandbox;

impl CommandSpec {
    /// Reconstructs and validates the complete existing semantic command format.
    ///
    /// This is data decoding, not installation or authority. The trusted receiver
    /// must bind the resulting CommandId to its independently installed boot
    /// expectation and resolve all program, root, resource and grant references
    /// through their existing owners. Enclosing codecs own input/depth budgets.
    ///
    /// # Errors
    /// Refuses wrong schemas, missing/extra/duplicate fields, invalid constructor
    /// inputs and any normalization that changes the complete semantic identity.
    pub fn from_datum(value: &Datum) -> Result<Self> {
        let (f, extensions) = match fields(value, "command-spec-v1", 11) {
            Ok(f) => (f, None),
            Err(_) => {
                let f = fields(value, "command-spec-v2", 13)?;
                (f, Some((field(f, "checkout")?, field(f, "manifest")?)))
            }
        };
        let spec = Self::new(
            ProgramRef::new(string(field(f, "program")?)?)?,
            ProjectRootRef::new(string(field(f, "root")?)?)?,
            invocation(field(f, "invocation")?)?,
            environment(field(f, "environment")?)?,
            vector(field(f, "resources")?)?
                .iter()
                .map(resource)
                .collect::<Result<_>>()?,
            budget(field(f, "budget")?)?,
            outputs(field(f, "outputs")?)?,
            cleanup(field(f, "cleanup")?)?,
            network(field(f, "network")?)?,
            sandbox::route(field(f, "route")?)?,
            match token(field(f, "replay")?, "operation")? {
                "idempotent" => CommandReplayPolicy::Idempotent,
                "exactly-once" => CommandReplayPolicy::ExactlyOnce,
                _ => return Err(invalid("replay policy")),
            },
        )?;
        let spec = match extensions {
            None => spec,
            Some((files, manifest)) => {
                let files = vector(files)?;
                let spec = if files.is_empty() {
                    spec
                } else {
                    spec.with_checkout(files.iter().map(checkout_file).collect::<Result<_>>()?)?
                };
                match manifest {
                    Datum::Nil => spec,
                    selection => spec.with_manifest_selection(manifest_selection(selection)?)?,
                }
            }
        };
        // Reject the schema before recursively canonicalizing unrecognized data.
        // Canonicalization still rejects duplicate unordered members even where
        // a constructor collects them into an ordered set.
        let original = value
            .canonical_bytes()
            .map_err(|_| invalid("canonical command"))?;
        if spec
            .canonical_datum()
            .canonical_bytes()
            .map_err(|_| invalid("canonical command"))?
            != original
        {
            return Err(invalid("command reconstruction changed semantic identity"));
        }
        Ok(spec)
    }
}

fn manifest_selection(value: &Datum) -> Result<ManifestSelection> {
    let f = fields(value, "manifest-selection-v1", 5)?;
    Ok(ManifestSelection {
        resource: string(field(f, "resource")?)?.to_owned(),
        path: string(field(f, "path")?)?.to_owned(),
        table: string(field(f, "table")?)?.to_owned(),
        name: string(field(f, "name")?)?.to_owned(),
        field: string(field(f, "field")?)?.to_owned(),
    })
}

fn checkout_file(value: &Datum) -> Result<CheckoutFile> {
    let f = fields(value, "checkout-file-v1", 3)?;
    Ok(CheckoutFile {
        resource: string(field(f, "resource")?)?.to_owned(),
        path: string(field(f, "path")?)?.to_owned(),
        target: string(field(f, "target")?)?.to_owned(),
    })
}

impl OutputContract {
    /// Reconstructs the exact semantic output contract independently of a
    /// complete command specification.
    ///
    /// This is a pure evidence boundary for observers that retain an intent's
    /// intended result. It grants no command-installation or execution authority.
    ///
    /// # Errors
    /// Refuses malformed, ambiguous, noncanonical or unbounded output data.
    pub fn from_datum(value: &Datum) -> Result<Self> {
        let contract = outputs(value)?;
        if contract
            .canonical_datum()
            .canonical_bytes()
            .map_err(|_| invalid("canonical output contract"))?
            != value
                .canonical_bytes()
                .map_err(|_| invalid("canonical output contract"))?
        {
            return Err(invalid("output contract reconstruction changed identity"));
        }
        Ok(contract)
    }
}

fn invocation(value: &Datum) -> Result<CommandInvocation> {
    match tag(value)? {
        "argv-v1" => {
            let f = fields(value, "argv-v1", 1)?;
            Ok(CommandInvocation::Argv(argv(field(f, "argv")?)?))
        }
        "interpreter-v1" => {
            let f = fields(value, "interpreter-v1", 2)?;
            Ok(CommandInvocation::Interpreter {
                flags: argv(field(f, "flags")?)?,
                script: bytes(field(f, "script")?)?.to_vec(),
            })
        }
        _ => Err(invalid("invocation")),
    }
}

fn argv(value: &Datum) -> Result<Vec<ArgAtom>> {
    vector(value)?
        .iter()
        .map(|value| ArgAtom::new(string(value)?))
        .collect()
}

fn environment(value: &Datum) -> Result<SealedBindings> {
    let f = fields(value, "environment-v1", 1)?;
    let Datum::Map(entries) = field(f, "bindings")? else {
        return Err(invalid("bindings map"));
    };
    let entries = entries
        .iter()
        .map(|(name, value)| {
            let name = string(name)?.to_owned();
            let kind = tag(value)?;
            let f = fields(value, kind, 1)?;
            let value = string(field(f, "value")?)?;
            let binding = match kind {
                "literal-v1" => BindingValue::Literal(value.into()),
                "project-root-v1" => BindingValue::ProjectRoot(ProjectRootRef::new(value)?),
                "private-artifact-v1" => {
                    BindingValue::PrivateArtifact(PrivateArtifactRef::new(value)?)
                }
                _ => return Err(invalid("binding kind")),
            };
            Ok((name, binding))
        })
        .collect::<Result<Vec<_>>>()?;
    SealedBindings::try_from_entries(entries)
}

fn resource(value: &Datum) -> Result<CommandResource> {
    let f = fields(value, "resource-v1", 3)?;
    Ok(CommandResource {
        source: string(field(f, "source")?)?.into(),
        guest_path: string(field(f, "guest-path")?)?.into(),
        access: match token(field(f, "access")?, "resource-access")? {
            "read-only" => ResourceAccess::ReadOnly,
            "writable" => ResourceAccess::Writable,
            _ => return Err(invalid("resource access")),
        },
    })
}

fn budget(value: &Datum) -> Result<ProcessBudget> {
    let f = fields(value, "budget-v1", 3)?;
    Ok(ProcessBudget {
        timeout_ms: unsigned(field(f, "timeout-ms")?)?,
        max_output_bytes: size(field(f, "max-output-bytes")?)?,
        stdin: match field(f, "stdin")? {
            Datum::Nil => None,
            value => Some(bytes(value)?.to_vec()),
        },
    })
}

fn outputs(value: &Datum) -> Result<OutputContract> {
    let f = fields(value, "output-contract-v1", 2)?;
    let codes = set(field(f, "exit-codes")?)?
        .iter()
        .map(|value| {
            let text = number(value, "i64")?;
            let code: i64 = text.parse().map_err(|_| invalid("exit code"))?;
            if code.to_string() != text {
                return Err(invalid("canonical exit code"));
            }
            i32::try_from(code).map_err(|_| invalid("exit code width"))
        })
        .collect::<Result<Vec<_>>>()?;
    let values = vector(field(f, "outputs")?)?
        .iter()
        .map(|value| {
            let f = fields(value, "output-v1", 3)?;
            let state = field(f, "state")?;
            let state = if matches!(state, Datum::Node { .. }) {
                OutputState::FileContent(content_id(state)?)
            } else {
                match token(state, "output")? {
                    "exists" => OutputState::Exists,
                    "absent" => OutputState::Absent,
                    _ => return Err(invalid("output state")),
                }
            };
            Ok(OutputExpectation {
                resource: string(field(f, "resource")?)?.into(),
                relative_path: string(field(f, "relative-path")?)?.into(),
                state,
            })
        })
        .collect::<Result<_>>()?;
    OutputContract::new(codes, values)
}

fn cleanup(value: &Datum) -> Result<CleanupContract> {
    let f = fields(value, "cleanup-contract-v1", 2)?;
    if token(field(f, "descendant-group")?, "cleanup")? != "kill-reap-required" {
        return Err(invalid("descendant cleanup"));
    }
    let resources = set(field(f, "scratch-resources")?)?
        .iter()
        .map(|value| Ok(string(value)?.to_owned()))
        .collect::<Result<Vec<_>>>()?;
    CleanupContract::process_group(resources)
}

fn network(value: &Datum) -> Result<NetworkAccess> {
    if matches!(value, Datum::Symbol(_)) {
        if token(value, "network")? != "absent" {
            return Err(invalid("network policy"));
        }
        return Ok(NetworkAccess::Absent);
    }
    let f = fields(value, "network-scoped-v1", 1)?;
    Ok(NetworkAccess::Scoped(CapabilityName::new(string(field(
        f,
        "capability",
    )?)?)))
}

fn vector(value: &Datum) -> Result<&[Datum]> {
    match value {
        Datum::Vector(items) => Ok(items),
        _ => Err(invalid("vector")),
    }
}
fn set(value: &Datum) -> Result<&[Datum]> {
    match value {
        Datum::Set(items) => Ok(items),
        _ => Err(invalid("set")),
    }
}
fn bytes(value: &Datum) -> Result<&[u8]> {
    match value {
        Datum::Bytes(bytes) => Ok(bytes),
        _ => Err(invalid("bytes")),
    }
}
fn tag(value: &Datum) -> Result<&str> {
    let Datum::Node { tag, .. } = value else {
        return Err(invalid("node"));
    };
    if tag != &Symbol::qualified("local-check", tag.name.as_ref()) {
        return Err(invalid("node namespace"));
    }
    Ok(tag.name.as_ref())
}
fn token<'a>(value: &'a Datum, namespace: &str) -> Result<&'a str> {
    let Datum::Symbol(symbol) = value else {
        return Err(invalid("symbol"));
    };
    if symbol != &Symbol::qualified(namespace, symbol.name.as_ref()) {
        return Err(invalid("symbol namespace"));
    }
    Ok(symbol.name.as_ref())
}
fn number<'a>(value: &'a Datum, kind: &str) -> Result<&'a str> {
    let Datum::Number(value) = value else {
        return Err(invalid("number"));
    };
    if value.domain != Symbol::qualified("numbers", kind) {
        return Err(invalid("number domain"));
    }
    Ok(&value.canonical)
}
fn unsigned(value: &Datum) -> Result<u64> {
    let text = number(value, "u64")?;
    let value: u64 = text.parse().map_err(|_| invalid("unsigned integer"))?;
    if value.to_string() != text {
        return Err(invalid("canonical unsigned integer"));
    }
    Ok(value)
}
fn size(value: &Datum) -> Result<usize> {
    usize::try_from(unsigned(value)?).map_err(|_| invalid("native size width"))
}
fn invalid(part: &str) -> Error {
    Error::Eval(format!("invalid command specification: {part}"))
}

#[cfg(test)]
#[path = "command_decode/tests.rs"]
mod tests;
