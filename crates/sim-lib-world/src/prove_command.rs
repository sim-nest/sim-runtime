// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loadable `sim prove` command.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Datum, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::WorldError;
use crate::command::{self, ModelDomainLoad};
use crate::grant::ModelDomainGrant;
use crate::prove::{self, Measured};

/// CLI verb contributed by this loaded library.
pub const PROVE_VERB: &str = "prove";

/// Loadable proof command library.
#[derive(Clone)]
pub struct ProveCommandLib {
    load: Option<ModelDomainLoad>,
}

impl Default for ProveCommandLib {
    fn default() -> Self {
        Self::new()
    }
}

impl ProveCommandLib {
    /// Constructs the command library without a model-domain load receipt.
    #[must_use]
    pub fn new() -> Self {
        Self { load: None }
    }

    /// Constructs the command library after a model-domain loader realized it.
    ///
    /// # Errors
    /// Returns an error when `load` omits the provider, loader, or artifact.
    pub fn for_model_load(load: ModelDomainLoad) -> std::result::Result<Self, WorldError> {
        require_field("provider", &load.provider)?;
        require_field("loader", &load.loader)?;
        require_field("artifact", &load.artifact)?;
        Ok(Self { load: Some(load) })
    }
}

fn require_field(name: &str, value: &str) -> std::result::Result<(), WorldError> {
    if value.is_empty() {
        return Err(WorldError::Qualification(format!(
            "model load {name} is empty"
        )));
    }
    Ok(())
}

impl Lib for ProveCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "prove-command"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: sim_run_core::cli_main_entrypoint_symbol(PROVE_VERB),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            sim_run_core::cli_main_entrypoint_symbol(PROVE_VERB),
            cx.factory().opaque(Arc::new(ProveEntrypoint {
                load: self.load.clone(),
            }))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct ProveEntrypoint {
    load: Option<ModelDomainLoad>,
}

impl Object for ProveEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/prove".into())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ProveEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ProveEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let argv = envelope_args(cx, args.values().first())?;
        if let Some(load) = &self.load {
            let grant = ModelDomainGrant::issue(load.clone()).map_err(eval_error)?;
            grant.admit().map_err(eval_error)?;
        }
        self.finish(cx, self.dispatch(&argv)?)
    }
}

enum Outcome {
    Value(Datum),
    Refused { call: String, reason: String },
    OverCeiling { value: Datum, command: &'static str },
}

impl ProveEntrypoint {
    fn dispatch(&self, argv: &[String]) -> Result<Outcome> {
        let outcome = match argv.get(1).map(String::as_str) {
            Some("plan") if argv.len() == 2 => prove::plan(None),
            Some("plan") if argv.len() == 3 => prove::plan(Some(&argv[2])),
            Some("run") if argv.len() == 3 => prove::run(&argv[2], None),
            Some("run") if argv.len() == 4 => prove::run(&argv[2], Some(&argv[3])),
            Some("status") if argv.len() == 2 => prove::status(None),
            Some("status") if argv.len() == 3 => prove::status(Some(&argv[2])),
            Some("why") if argv.len() == 4 => prove::why(&argv[2], &argv[3], None),
            Some("why") if argv.len() == 5 => prove::why(&argv[2], &argv[3], Some(&argv[4])),
            Some("frontier") if argv.len() == 2 => prove::frontier(None),
            Some("frontier")
                if argv.len() == 3 && argv[2].starts_with("sim-world-proof-view-v1") =>
            {
                prove::frontier(Some(&argv[2]))
            }
            Some("frontier") if argv.len() == 3 => prove::frontier_leaf(&argv[2], None),
            Some("frontier") if argv.len() == 4 => prove::frontier_leaf(&argv[2], Some(&argv[3])),
            Some("resume") if argv.len() == 3 => prove::resume(&argv[2], None),
            Some("resume") if argv.len() == 4 => prove::resume(&argv[2], Some(&argv[3])),
            Some("measure") if argv.len() == 3 && matches!(argv[2].as_str(), "status" | "why") => {
                return measured(&argv[2], prove::measure(&argv[2]).map_err(eval_error)?);
            }
            _ => {
                return Ok(Outcome::Refused {
                    call: "usage".to_owned(),
                    reason: "usage".to_owned(),
                });
            }
        };
        outcome.map(Outcome::Value).map_err(eval_error)
    }

    fn finish(&self, cx: &mut Cx, outcome: Outcome) -> Result<Value> {
        match outcome {
            Outcome::Value(value) => {
                self.emit(&value);
                cx.factory().bool(true)
            }
            Outcome::OverCeiling { value, command } => {
                self.emit(&value);
                Err(Error::Eval(format!(
                    "prove cached {command} p95 exceeded 200 ms"
                )))
            }
            Outcome::Refused { call, reason } => {
                println!("{}", command::render(&refusal(&call, &reason)));
                let detail = if reason == "usage" {
                    USAGE.to_owned()
                } else {
                    format!("prove refused {call}: {reason}")
                };
                Err(Error::Eval(detail))
            }
        }
    }

    fn emit(&self, value: &Datum) {
        println!("{}", command::render(value));
        if let Some(load) = &self.load {
            println!("{}", command::render(&load_receipt(load)));
        }
    }
}

fn measured(command: &str, measured: Measured) -> Result<Outcome> {
    if measured.within_ceiling {
        Ok(Outcome::Value(measured.record))
    } else {
        let command = match command {
            "status" => "status",
            "why" => "why",
            _ => "query",
        };
        Ok(Outcome::OverCeiling {
            value: measured.record,
            command,
        })
    }
}

fn refusal(call: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("prove", "refusal-v1"),
        fields: vec![
            (Symbol::new("call"), Datum::String(call.to_owned())),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
        ],
    }
}

fn load_receipt(load: &ModelDomainLoad) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("prove", "load-receipt-v1"),
        fields: vec![
            (
                Symbol::new("provider"),
                Datum::String(load.provider.clone()),
            ),
            (Symbol::new("loader"), Datum::String(load.loader.clone())),
            (
                Symbol::new("artifact"),
                Datum::String(load.artifact.clone()),
            ),
        ],
    }
}

fn eval_error(error: WorldError) -> Error {
    Error::Eval(error.to_string())
}

fn envelope_args(cx: &mut Cx, envelope: Option<&Value>) -> Result<Vec<String>> {
    let envelope = envelope.ok_or_else(|| Error::Eval("missing prove envelope".into()))?;
    let table = envelope
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("prove envelope is not a table".into()))?;
    let value = table.get(cx, Symbol::new("args"))?;
    let Expr::List(values) = value.object().as_expr(cx)? else {
        return Err(Error::Eval("prove args are not a list".into()));
    };
    values
        .into_iter()
        .map(|value| match value {
            Expr::String(value) => Ok(value),
            _ => Err(Error::Eval("prove arg is not a string".into())),
        })
        .collect()
}

const USAGE: &str = "usage: sim prove plan [CONTINUATION] | run cold|D1|D2|D3|D4|D5|D6 [VIEW] | status [VIEW] | why DELTA LEAF [VIEW] | frontier [LEAF|VIEW] [VIEW] | resume CONTINUATION [VIEW] | measure status|why";
