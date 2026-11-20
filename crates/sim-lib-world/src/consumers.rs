// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Portable consumers of the loaded world entry.
//!
//! Project, diff, why, read-only, and the revocation sequence run through
//! `cli/main/world` on a loader session. A direct dispatch result is not
//! that entry. The index lookup is the same route query the index entry renders.

use std::ffi::OsString;

use sim_kernel::{
    AbiVersion, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Result, Symbol, Version,
};
use sim_lib_index::{RouteMatch, load_embedded_index_doc, route};
use sim_run_core::{
    CliBoot, DEFAULT_CODEC_NAME, LibSourceSpec, LoadReceiptRole, LoadSession, Payload,
    codec_lib_symbol,
};

use crate::WorldError;
use crate::command::{ModelDomainLoad, WorldCommandLib, recorded_lines, render};

const KIND: &str = "world/public-api-v1";
const FACT: &str = "source/public-api";
const CONCLUSION: &str = "conclusion/source-api";
const INPUT: &str = "pub fn project()";
const SHAPE: &str = "(shape \"world/config-v1\")";
const PROVIDER: &str = "(provider \"platform/site/model\")";
const ENTRY: &str = "cli/main/world";
const EXPLAIN_TASK: &str = "Project supplied world facts and explain their exact affected closure";
const EXPLAIN_ROUTE: &str = "route/explain-semantic-world-change";
const COMMAND_TASK: &str =
    "Project supplied semantic facts and explain their exact affected closure";
const COMMAND_ROUTE: &str = "route/run-world-command";
const WORLD_FEATURE: &str = "feature/sim-runtime/world-projection-product";
const WORLD_RECIPE: &str = "recipe/sim-runtime/crates/sim-lib-world/01-projection/world-explain";

/// Text captured from one loaded world entry, with its session export check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedConsumers {
    /// Projection printed by the loaded project call.
    pub project: String,
    /// Diff of two equal semantic values.
    pub same_diff: String,
    /// Diff of two different semantic values.
    pub changed_diff: String,
    /// Explanation printed by the loaded why call.
    pub why: String,
    /// Read-only custody printed by the loaded command.
    pub read_only: String,
    /// Revoke, teardown, and reopen printed by one loaded compose call.
    pub compose: String,
    /// World routes returned by the embedded index query.
    pub index: String,
}

/// Runs the portable consumers through the loaded world entry.
///
/// # Errors
///
/// Returns an error when the session cannot load the entry, a call exits
/// non-zero, or a rendered result misses its expected field.
pub fn execute() -> Result<LoadedConsumers, WorldError> {
    let project = session_output(&["world", "project", KIND, FACT, INPUT])?;
    require(&project, &format!("(kind \"{KIND}\")"), "project kind")?;
    require(&project, &format!("(fact \"{FACT}\")"), "project fact")?;
    require(&project, SHAPE, "project shape")?;
    require(&project, PROVIDER, "project provider")?;
    require(&project, "load-receipt-v1", "project receipt")?;

    let same_diff = session_output(&["world", "diff", KIND, FACT, "one", "one"])?;
    require(&same_diff, "(changed false)", "equal diff")?;
    let changed_diff = session_output(&["world", "diff", KIND, FACT, "one", "two"])?;
    require(&changed_diff, "(changed true)", "changed diff")?;

    let why = session_output(&["world", "why", CONCLUSION, FACT])?;
    require(&why, CONCLUSION, "why conclusion")?;
    require(&why, FACT, "why fact")?;

    let read_only = session_output(&["world", "read-only"])?;
    require(&read_only, "(effects 0)", "read-only effects")?;
    require(&read_only, "(resident true)", "read-only residency")?;
    require(&read_only, "(capabilities [])", "read-only capabilities")?;

    let compose = session_output(&[
        "world", "compose", "project", KIND, FACT, INPUT, "revoke", "project", KIND, FACT, INPUT,
        "teardown", "project", KIND, FACT, INPUT, "reopen", "project", KIND, FACT, INPUT,
    ])?;
    require_after(
        &compose,
        "(disposition \"current\")",
        "RevocationUnknownOrActive",
    )?;
    require_after(
        &compose,
        "RevocationUnknownOrActive",
        "checker owner is not live",
    )?;
    require_after(&compose, "checker owner is not live", "(call \"reopen\")")?;
    require_after(&compose, "(call \"reopen\")", "(disposition \"current\")")?;

    let index = index_routes()?;

    Ok(LoadedConsumers {
        project,
        same_diff,
        changed_diff,
        why,
        read_only,
        compose,
        index,
    })
}

/// Rejects a direct dispatch as a substitute for the loaded entry.
///
/// The helper may return a datum. That datum is admitted only when it meets
/// the loaded project expectations. The first missing field is the refusal.
///
/// # Errors
///
/// Returns an error when the direct datum misses a loaded-entry field, when
/// the direct call fails, or when that datum carries a session load receipt.
pub fn helper_dispatch_is_refused() -> Result<(), WorldError> {
    let command = WorldCommandLib::for_model_load(model_load())?;
    let value = command.run(&[
        "world".to_owned(),
        "project".to_owned(),
        KIND.to_owned(),
        FACT.to_owned(),
        INPUT.to_owned(),
    ])?;
    let rendered = render(&value);
    require(&rendered, &format!("(kind \"{KIND}\")"), "project kind")?;
    require(&rendered, &format!("(fact \"{FACT}\")"), "project fact")?;
    require(&rendered, SHAPE, "project shape")?;
    require(&rendered, PROVIDER, "project provider")?;
    require(&rendered, "load-receipt-v1", "project receipt")?;
    Err(WorldError::Qualification(
        "helper dispatch was accepted".to_owned(),
    ))
}

/// Refuses a project whose fact is not one of the bundled pairs.
///
/// # Errors
///
/// Returns an error when the loaded entry accepts the unknown fact.
pub fn unknown_fact_is_refused() -> Result<(), WorldError> {
    match session_output(&["world", "project", KIND, "fact/not-a-consumer", INPUT]) {
        Ok(text) => Err(WorldError::Qualification(format!(
            "unknown fact was projected: {text}"
        ))),
        Err(error) if error.to_string().contains("unsupported-pair") => Ok(()),
        Err(error) => Err(error),
    }
}

/// Refuses an unrelated index task as a world route.
///
/// # Errors
///
/// Returns an error when that task still resolves to a world route.
pub fn unrelated_index_task_is_refused() -> Result<(), WorldError> {
    let doc = load_embedded_index_doc()
        .map_err(|error| WorldError::Qualification(format!("index snapshot: {error}")))?;
    let rows = route(&doc, "add a codec");
    if rows
        .iter()
        .any(|row| row.id == EXPLAIN_ROUTE || row.id == COMMAND_ROUTE)
    {
        return Err(WorldError::Qualification(
            "unrelated index task returned the world route".to_owned(),
        ));
    }
    Ok(())
}

fn index_routes() -> Result<String, WorldError> {
    let doc = load_embedded_index_doc()
        .map_err(|error| WorldError::Qualification(format!("index snapshot: {error}")))?;
    let explain = route(&doc, EXPLAIN_TASK);
    let explain_row = expect_route(&explain, EXPLAIN_ROUTE, true)?;
    let command = route(&doc, COMMAND_TASK);
    let command_row = expect_route(&command, COMMAND_ROUTE, false)?;
    Ok(format!("{}\n{}", explain_row.id, command_row.id))
}

fn expect_route<'a>(
    rows: &'a [RouteMatch],
    route_id: &str,
    require_top: bool,
) -> Result<&'a RouteMatch, WorldError> {
    if require_top {
        let Some(first) = rows.first() else {
            return Err(WorldError::Qualification(format!(
                "index route {route_id} missing"
            )));
        };
        if first.id != route_id {
            return Err(WorldError::Qualification(format!(
                "index route {route_id} was not first: {}",
                first.id
            )));
        }
    }
    let Some(row) = rows.iter().find(|row| row.id == route_id) else {
        return Err(WorldError::Qualification(format!(
            "index route {route_id} missing"
        )));
    };
    if !row
        .steps
        .iter()
        .any(|step| step.kind == "feature" && step.id == WORLD_FEATURE)
    {
        return Err(WorldError::Qualification(format!(
            "index route {route_id} missing {WORLD_FEATURE}"
        )));
    }
    if !row
        .steps
        .iter()
        .any(|step| step.kind == "specimen" && step.id == WORLD_RECIPE)
    {
        let steps = row
            .steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(WorldError::Qualification(format!(
            "index route {route_id} missing {WORLD_RECIPE}: {steps}"
        )));
    }
    Ok(row)
}

fn require(text: &str, needle: &str, reason: &str) -> Result<(), WorldError> {
    if text.contains(needle) {
        Ok(())
    } else {
        Err(WorldError::Qualification(format!(
            "{reason} missing {needle}: {text}"
        )))
    }
}

fn require_after(text: &str, earlier: &str, later: &str) -> Result<(), WorldError> {
    let Some(start) = text.find(earlier) else {
        return Err(WorldError::Qualification(format!(
            "missing {earlier}: {text}"
        )));
    };
    if text[start + earlier.len()..].contains(later) {
        Ok(())
    } else {
        Err(WorldError::Qualification(format!(
            "{later} did not follow {earlier}: {text}"
        )))
    }
}

fn model_load() -> ModelDomainLoad {
    ModelDomainLoad {
        provider: "platform/site/model".to_owned(),
        loader: "loader/static-v1".to_owned(),
        artifact: "lib/world-command".to_owned(),
    }
}

fn session_codec_key() -> String {
    codec_lib_symbol(DEFAULT_CODEC_NAME)
}

fn session_codec_symbol() -> Symbol {
    let key = session_codec_key();
    let (namespace, name) = key.split_once('/').expect("boot codec symbol");
    Symbol::qualified(namespace, name)
}

struct SessionBootHost;

impl Lib for SessionBootHost {
    fn manifest(&self) -> LibManifest {
        let symbol = session_codec_symbol();
        LibManifest {
            id: symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol,
                codec_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.codec_value(session_codec_symbol(), cx.factory().bool(true)?)?;
        Ok(())
    }
}

fn session_output(args: &[&str]) -> Result<String, WorldError> {
    let captured = capture_entry(args)?;
    if captured.code != 0 {
        return Err(WorldError::Qualification(format!(
            "world entry exited {}: {}",
            captured.code, captured.text
        )));
    }
    if !captured.entry_exported {
        return Err(WorldError::Qualification(
            "world entry was not loaded".to_owned(),
        ));
    }
    Ok(captured.text)
}

struct CapturedEntry {
    code: i32,
    text: String,
    entry_exported: bool,
}

fn capture_entry(args: &[&str]) -> Result<CapturedEntry, WorldError> {
    let boot = CliBoot {
        payload: Payload {
            args: args.iter().copied().map(OsString::from).collect(),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let codec = session_codec_key();
    let mut session =
        LoadSession::with_cache_root(std::env::temp_dir().join("sim-lib-world-consumers"))
            .with_host_factory(codec.clone(), || Box::new(SessionBootHost))
            .with_host_factory("lib/world-command", || {
                Box::new(
                    WorldCommandLib::for_model_load(model_load())
                        .expect("model load fields are present"),
                )
            })
            .with_default_verb_sources(
                "world",
                vec![
                    LibSourceSpec::Host(codec),
                    LibSourceSpec::Host("lib/world-command".to_owned()),
                ],
            );
    let (result, text) = recorded_lines(|| session.run_loaded_boot(&boot));
    let (code, entry_exported) = match result {
        Ok(code) => (code, entry_exported(&session)),
        Err(error) => {
            return Err(WorldError::Qualification(format!("{error}: {text}")));
        }
    };
    Ok(CapturedEntry {
        code,
        text,
        entry_exported,
    })
}

fn entry_exported(session: &LoadSession) -> bool {
    session.receipts().iter().any(|receipt| {
        matches!(receipt.role, LoadReceiptRole::Library)
            && receipt.manifest.exports.iter().any(|export| {
                matches!(
                    export,
                    Export::Function { symbol, .. } if symbol.to_string() == ENTRY
                )
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loaded_entry_meets_the_consumer_expectations() {
        let loaded = execute().expect("loaded consumers");
        assert!(loaded.project.contains(SHAPE));
        assert!(loaded.same_diff.contains("(changed false)"));
        assert!(loaded.changed_diff.contains("(changed true)"));
        assert!(loaded.why.contains(CONCLUSION));
        assert!(loaded.read_only.contains("(effects 0)"));
        assert!(loaded.compose.contains("RevocationUnknownOrActive"));
        assert!(loaded.compose.contains("checker owner is not live"));
        assert!(loaded.index.contains(EXPLAIN_ROUTE));
        assert!(loaded.index.contains(COMMAND_ROUTE));
    }

    #[test]
    fn an_unrelated_index_task_is_not_the_world_route() {
        unrelated_index_task_is_refused().expect("unrelated index task refused");
    }

    #[test]
    fn a_direct_dispatch_is_refused() {
        let error = helper_dispatch_is_refused().expect_err("helper stood in for the loaded entry");
        let text = error.to_string();
        assert!(text.contains("project provider missing"), "{text}");
        assert!(!text.contains("load-receipt-v1"), "{text}");
    }

    #[test]
    fn an_unknown_fact_is_refused() {
        unknown_fact_is_refused().expect("unknown fact refused");
    }
}
