// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Datum, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::grant::{self, ModelDomainGrant};
use crate::query;
use crate::{WorldError, WorldProduct};

/// CLI verb contributed by this loaded library.
pub const WORLD_VERB: &str = "world";

/// Model-domain load that realized this command library.
///
/// The provider, loader, and artifact identify the typed installation of
/// this trusted compiled library. They are recorded on the command receipt.
/// They are not a projection result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelDomainLoad {
    /// Model provider site that owned the load.
    pub provider: String,
    /// Loader kind that realized the trusted artifact.
    pub loader: String,
    /// Artifact symbol registered with that loader.
    pub artifact: String,
}

/// Loadable read-only `sim world` command library.
#[derive(Clone)]
pub struct WorldCommandLib {
    product: WorldProduct,
    load: Option<ModelDomainLoad>,
}

impl WorldCommandLib {
    /// Constructs the command library with the qualified bundled providers.
    pub fn new() -> std::result::Result<Self, crate::WorldError> {
        Ok(Self {
            product: WorldProduct::bundled()?,
            load: None,
        })
    }

    /// Constructs the command library after a model-domain loader realized it.
    ///
    /// # Errors
    /// Returns an error when the bundled providers fail to qualify, or when
    /// `load` omits the provider, loader, or artifact.
    pub fn for_model_load(load: ModelDomainLoad) -> std::result::Result<Self, crate::WorldError> {
        require_load_field("provider", &load.provider)?;
        require_load_field("loader", &load.loader)?;
        require_load_field("artifact", &load.artifact)?;
        Ok(Self {
            product: WorldProduct::bundled()?,
            load: Some(load),
        })
    }
}

fn require_load_field(name: &str, value: &str) -> std::result::Result<(), crate::WorldError> {
    if value.is_empty() {
        return Err(crate::WorldError::Qualification(format!(
            "model load {name} is empty"
        )));
    }
    Ok(())
}

impl Lib for WorldCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "world-command"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: sim_run_core::cli_main_entrypoint_symbol(WORLD_VERB),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            sim_run_core::cli_main_entrypoint_symbol(WORLD_VERB),
            cx.factory().opaque(Arc::new(WorldEntrypoint {
                product: self.product.clone(),
                load: self.load.clone(),
            }))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct WorldEntrypoint {
    product: WorldProduct,
    load: Option<ModelDomainLoad>,
}

impl Object for WorldEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/world".into())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for WorldEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for WorldEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let argv = envelope_args(cx, args.values().first())?;
        if argv.get(1).map(String::as_str) == Some("compose") {
            let load = self.load.clone().ok_or_else(|| {
                Error::Eval("world compose requires a model-domain load".to_owned())
            })?;
            let mut grant = ModelDomainGrant::issue(load).map_err(eval_error)?;
            compose(&self.product, &mut grant, &argv[2..])?;
            return cx.factory().bool(true);
        }
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
    OverCeiling(Datum),
}

impl WorldEntrypoint {
    fn dispatch(&self, argv: &[String]) -> Result<Outcome> {
        match argv.get(1).map(String::as_str) {
            Some("project") if argv.len() == 5 => {
                self.project_outcome(&argv[2], &argv[3], &argv[4])
            }
            Some("diff") if argv.len() == 6 => self
                .product
                .diff(
                    &argv[2],
                    &argv[3],
                    Datum::String(argv[4].clone()),
                    Datum::String(argv[5].clone()),
                )
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("why") if argv.len() == 4 => self
                .product
                .why(&argv[2], &argv[3])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("read-only") if argv.len() == 2 => Ok(Outcome::Value(self.read_only_value())),
            Some("measure")
                if argv.len() == 3 && argv.get(2).map(String::as_str) == Some("why") =>
            {
                self.measure_outcome()
            }
            _ => Ok(Outcome::Refused {
                call: "usage".to_owned(),
                reason: "usage".to_owned(),
            }),
        }
    }

    fn project_outcome(&self, kind: &str, fact: &str, value: &str) -> Result<Outcome> {
        match self
            .product
            .project(kind, fact, Datum::String(value.to_owned()), None)
        {
            Ok(projection) => Ok(Outcome::Value(projection.value)),
            Err(WorldError::UnsupportedPair { .. }) => Ok(Outcome::Refused {
                call: "project".to_owned(),
                reason: "unsupported-pair".to_owned(),
            }),
            Err(error) => Err(eval_error(error)),
        }
    }

    fn measure_outcome(&self) -> Result<Outcome> {
        let measured = query::measure_why(self.product.effect_calls()).map_err(eval_error)?;
        if measured.within_ceiling {
            Ok(Outcome::Value(measured.record))
        } else {
            Ok(Outcome::OverCeiling(measured.record))
        }
    }

    fn read_only_value(&self) -> Datum {
        let manifest = WorldCommandLib {
            product: self.product.clone(),
            load: self.load.clone(),
        }
        .manifest();
        query::read_only_record(
            self.product.effect_calls(),
            &manifest.capabilities,
            &manifest.requires,
        )
    }

    fn finish(&self, cx: &mut Cx, outcome: Outcome) -> Result<Value> {
        match outcome {
            Outcome::Value(value) => {
                self.emit(&value);
                cx.factory().bool(true)
            }
            Outcome::OverCeiling(value) => {
                self.emit(&value);
                Err(Error::Eval(
                    "world cached why p95 exceeded 200 ms".to_owned(),
                ))
            }
            Outcome::Refused { call, reason } => {
                println!("{}", render(&refusal(&call, &reason)));
                let detail = if reason == "usage" {
                    USAGE.to_owned()
                } else {
                    format!("world refused {call}: {reason}")
                };
                Err(Error::Eval(detail))
            }
        }
    }

    fn emit(&self, value: &Datum) {
        println!("{}", render(value));
        if let Some(load) = &self.load {
            println!("{}", render(&load_receipt(load)));
        }
    }
}

const USAGE: &str = "usage: sim world project KIND FACT VALUE | diff KIND FACT BEFORE AFTER | why CONCLUSION FACT | compose STEPS | read-only | measure why";

fn refusal(call: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "refusal-v1"),
        fields: vec![
            (Symbol::new("call"), Datum::String(call.to_owned())),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
        ],
    }
}

fn eval_error(error: WorldError) -> Error {
    Error::Eval(error.to_string())
}

fn compose(product: &WorldProduct, grant: &mut ModelDomainGrant, args: &[String]) -> Result<()> {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "revoke" => {
                grant.revoke().map_err(eval_error)?;
                println!("{}", render(&custody("revoke")));
                index += 1;
            }
            "teardown" => {
                grant.teardown();
                println!("{}", render(&custody("teardown")));
                index += 1;
            }
            "reopen" => {
                grant.reopen().map_err(eval_error)?;
                println!("{}", render(&custody("reopen")));
                index += 1;
            }
            "project" => {
                let kind = step_arg(args, index + 1)?;
                let fact = step_arg(args, index + 2)?;
                let value = step_arg(args, index + 3)?;
                emit_use(grant, "project", || {
                    product
                        .project(kind, fact, Datum::String(value.to_owned()), None)
                        .map(|projection| projection.value)
                })?;
                index += 4;
            }
            "diff" => {
                let kind = step_arg(args, index + 1)?;
                let fact = step_arg(args, index + 2)?;
                let before = step_arg(args, index + 3)?;
                let after = step_arg(args, index + 4)?;
                emit_use(grant, "diff", || {
                    product.diff(
                        kind,
                        fact,
                        Datum::String(before.to_owned()),
                        Datum::String(after.to_owned()),
                    )
                })?;
                index += 5;
            }
            "why" => {
                let conclusion = step_arg(args, index + 1)?;
                let fact = step_arg(args, index + 2)?;
                emit_use(grant, "why", || product.why(conclusion, fact))?;
                index += 3;
            }
            _ => return Err(Error::Eval(USAGE.to_owned())),
        }
    }
    Ok(())
}

fn step_arg(args: &[String], index: usize) -> Result<&str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Error::Eval(USAGE.to_owned()))
}

fn emit_use(
    grant: &ModelDomainGrant,
    call: &str,
    run: impl FnOnce() -> std::result::Result<Datum, WorldError>,
) -> Result<()> {
    let record = match grant.admit() {
        Ok(()) => use_record(
            grant,
            call,
            "current",
            Some(run().map_err(eval_error)?),
            None,
        ),
        Err(error) => use_record(
            grant,
            call,
            "refused",
            None,
            Some(grant::refusal_reason(&error)),
        ),
    };
    println!("{}", render(&record));
    Ok(())
}

fn use_record(
    grant: &ModelDomainGrant,
    call: &str,
    disposition: &str,
    value: Option<Datum>,
    reason: Option<String>,
) -> Datum {
    let load = grant.load();
    let mut fields = vec![
        (Symbol::new("call"), Datum::String(call.to_owned())),
        (
            Symbol::new("disposition"),
            Datum::String(disposition.to_owned()),
        ),
        (
            Symbol::new("provider"),
            Datum::String(load.provider.clone()),
        ),
        (Symbol::new("loader"), Datum::String(load.loader.clone())),
        (
            Symbol::new("artifact"),
            Datum::String(load.artifact.clone()),
        ),
    ];
    if let Some(value) = value {
        fields.push((Symbol::new("value"), value));
    }
    if let Some(reason) = reason {
        fields.push((Symbol::new("reason"), Datum::String(reason)));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "use-v1"),
        fields,
    }
}

fn custody(call: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "custody-v1"),
        fields: vec![
            (Symbol::new("call"), Datum::String(call.to_owned())),
            (Symbol::new("disposition"), Datum::String("done".to_owned())),
        ],
    }
}

fn load_receipt(load: &ModelDomainLoad) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "load-receipt-v1"),
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

fn envelope_args(cx: &mut Cx, envelope: Option<&Value>) -> Result<Vec<String>> {
    let envelope = envelope.ok_or_else(|| Error::Eval("missing world envelope".into()))?;
    let table = envelope
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("world envelope is not a table".into()))?;
    let value = table.get(cx, Symbol::new("args"))?;
    let Expr::List(values) = value.object().as_expr(cx)? else {
        return Err(Error::Eval("world args are not a list".into()));
    };
    values
        .into_iter()
        .map(|value| match value {
            Expr::String(value) => Ok(value),
            _ => Err(Error::Eval("world arg is not a string".into())),
        })
        .collect()
}

pub(crate) fn render(value: &Datum) -> String {
    match value {
        Datum::Nil => "nil".to_owned(),
        Datum::Bool(value) => value.to_string(),
        Datum::Number(value) => value.canonical.clone(),
        Datum::String(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        Datum::Symbol(value) => value.to_string(),
        Datum::Vector(values) => format!(
            "[{}]",
            values.iter().map(render).collect::<Vec<_>>().join(" ")
        ),
        Datum::List(values) => format!(
            "({})",
            values.iter().map(render).collect::<Vec<_>>().join(" ")
        ),
        Datum::Node { tag, fields } => {
            let fields = fields
                .iter()
                .map(|(name, value)| format!("({} {})", name, render(value)))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({tag} {fields})")
        }
        other => format!("{other:?}"),
    }
}
