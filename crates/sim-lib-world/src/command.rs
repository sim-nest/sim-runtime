// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Datum, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::canary;
use crate::deliver;
use crate::grant::{self, ModelDomainGrant};
use crate::observe;
use crate::plan::{self, View};
use crate::product::Declaration;
use crate::query;
use crate::release;
use crate::seal;
use crate::service;
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

    /// Loads caller declarations and records `load` on the same command path.
    ///
    /// # Errors
    /// Returns an error when `load` omits a field or a declaration is rejected.
    pub fn declared(
        load: ModelDomainLoad,
        declarations: Vec<Declaration>,
    ) -> std::result::Result<Self, crate::WorldError> {
        require_load_field("provider", &load.provider)?;
        require_load_field("loader", &load.loader)?;
        require_load_field("artifact", &load.artifact)?;
        Ok(Self {
            product: WorldProduct::declared(declarations)?,
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
    OverCeiling { value: Datum, command: &'static str },
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
            Some("measure") if argv.len() == 3 => match argv[2].as_str() {
                "why" => self.measure_outcome(),
                "plan" => self.measure_plan_outcome(),
                "seal" => self.measure_seal_outcome(),
                _ => Ok(Outcome::Refused {
                    call: "usage".to_owned(),
                    reason: "usage".to_owned(),
                }),
            },
            Some("seal") => self.seal_command(&argv[2..], false),
            Some("universe") => self.seal_command(&argv[2..], true),
            Some("seal-diff") => self.seal_diff_command(&argv[2..]),
            Some("assay") if argv.len() == 2 => self.assay_outcome(),
            Some("snapshot") if argv.len() == 4 => observe::snapshot_document(&argv[2], &argv[3])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("snapshot-diff") if argv.len() == 5 => {
                observe::diff_documents(&argv[2], &argv[3], &argv[4])
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("service") if argv.len() == 4 => service::snapshot(&argv[2], &argv[3])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("service-plan") if argv.len() == 3 => service::plan_catalog(&argv[2])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("service-plan") if argv.len() == 5 => {
                service::plan_one(&argv[2], &argv[3], &argv[4])
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("release-plan") if argv.len() == 3 => release::plan_catalog(&argv[2])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("release-plan") if argv.len() == 5 => {
                release::plan_one(&argv[2], &argv[3], &argv[4])
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("release-status") if argv.len() == 3 => release::status(&argv[2])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("release-why") if argv.len() == 4 => release::why(&argv[2], &argv[3])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("deliver") if argv.len() == 3 => deliver::catalog(&argv[2])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("deliver") if argv.len() == 5 => deliver::one(&argv[2], &argv[3], &argv[4])
                .map(Outcome::Value)
                .map_err(eval_error),
            Some("deliver-status") if matches!(argv.len(), 3 | 4) => {
                deliver::status(&argv[2], argv.get(3).map(String::as_str))
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("deliver-why") if matches!(argv.len(), 4 | 5) => {
                deliver::why(&argv[2], &argv[3], argv.get(4).map(String::as_str))
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("canary") if argv.len() == 6 => {
                canary::run(&argv[2], &argv[3], &argv[4], &argv[5], "issued")
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("canary") if argv.len() == 7 => {
                canary::run(&argv[2], &argv[3], &argv[4], &argv[5], &argv[6])
                    .map(Outcome::Value)
                    .map_err(eval_error)
            }
            Some("plan") => self.plan_outcome(&argv[2..]),
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
            Ok(Outcome::OverCeiling {
                value: measured.record,
                command: "why",
            })
        }
    }

    fn measure_seal_outcome(&self) -> Result<Outcome> {
        seal::measure(self.product.effect_calls())
            .map(Outcome::Value)
            .map_err(eval_error)
    }

    fn seal_command(&self, args: &[String], universe: bool) -> Result<Outcome> {
        let parsed = match args {
            [] => Some((None, None)),
            [batch] => Some((Some(batch.as_str()), None)),
            [batch, target] => Some((Some(batch.as_str()), Some(target.as_str()))),
            _ => None,
        };
        let Some((batch, target)) = parsed else {
            return Ok(Outcome::Refused {
                call: "usage".to_owned(),
                reason: "usage".to_owned(),
            });
        };
        let value = if universe {
            seal::universe_admitted(batch, target)
        } else {
            seal::open_admitted(batch, target)
        };
        value.map(Outcome::Value).map_err(eval_error)
    }

    fn seal_diff_command(&self, args: &[String]) -> Result<Outcome> {
        let parsed = match args {
            [before, after] => Some((before.as_str(), after.as_str(), None)),
            [before, after, target] => {
                Some((before.as_str(), after.as_str(), Some(target.as_str())))
            }
            _ => None,
        };
        let Some((before, after, target)) = parsed else {
            return Ok(Outcome::Refused {
                call: "usage".to_owned(),
                reason: "usage".to_owned(),
            });
        };
        seal::diff_admitted(before, after, target)
            .map(Outcome::Value)
            .map_err(eval_error)
    }

    fn measure_plan_outcome(&self) -> Result<Outcome> {
        let measured = plan::measure(self.product.effect_calls()).map_err(eval_error)?;
        if measured.within_ceiling {
            Ok(Outcome::Value(measured.record))
        } else {
            Ok(Outcome::OverCeiling {
                value: measured.record,
                command: "plan",
            })
        }
    }

    fn plan_outcome(&self, args: &[String]) -> Result<Outcome> {
        let admitted = |text: &str| text.starts_with("sim-world-plan-v1");
        let parsed = match args {
            [] => Some((View::Expression, None, None)),
            [text] if admitted(text) => Some((View::Expression, None, Some(text.as_str()))),
            [view] if view == "text" => Some((View::Text, None, None)),
            [view] if view == "card" => Some((View::Card, None, None)),
            [view, text] if view == "text" && admitted(text) => {
                Some((View::Text, None, Some(text.as_str())))
            }
            [view, text] if view == "card" && admitted(text) => {
                Some((View::Card, None, Some(text.as_str())))
            }
            [view, token] if view == "text" => Some((View::Text, Some(token.as_str()), None)),
            [view, token] if view == "card" => Some((View::Card, Some(token.as_str()), None)),
            [text, token] if admitted(text) => {
                Some((View::Expression, Some(token.as_str()), Some(text.as_str())))
            }
            [token] => Some((View::Expression, Some(token.as_str()), None)),
            _ => None,
        };
        let Some((view, token, text)) = parsed else {
            return Ok(Outcome::Refused {
                call: "usage".to_owned(),
                reason: "usage".to_owned(),
            });
        };
        let opened = if let Some(text) = text {
            plan::open_admitted(text, view, token)
        } else {
            plan::open(view, token)
        };
        opened.map(Outcome::Value).map_err(eval_error)
    }

    fn assay_outcome(&self) -> Result<Outcome> {
        crate::assay::observe()
            .map(|observation| Outcome::Value(crate::assay::observation_record(&observation)))
            .map_err(eval_error)
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
            Outcome::OverCeiling { value, command } => {
                self.emit(&value);
                Err(Error::Eval(format!(
                    "world cached {command} p95 exceeded 200 ms"
                )))
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

const USAGE: &str = "usage: sim world project KIND FACT VALUE | diff KIND FACT BEFORE AFTER | why CONCLUSION FACT | compose STEPS | read-only | measure why | measure plan | measure seal | assay | plan [text|card] [CONTINUATION] | snapshot DOCUMENT SELECTOR | snapshot-diff BEFORE AFTER SELECTOR | service REALIZATION SELECTOR | service-plan REALIZATION [SELECTOR DESIRED] | release-plan REALIZATION [SELECTOR DESIRED] | release-status REALIZATION | release-why REALIZATION SELECTOR | deliver REALIZATION [SELECTOR DESIRED] | deliver-status REALIZATION [VIEW] | deliver-why REALIZATION SELECTOR [VIEW] | canary REALIZATION SOURCE DESTINATION PURPOSE [issued|spent] | seal [DOCUMENT [TARGET]] | universe [DOCUMENT [TARGET]] | seal-diff BEFORE AFTER [TARGET]";

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
        Datum::String(value) => {
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        }
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

/// Runs one shipped `sim world` argument list through the loaded entrypoint.
///
/// # Errors
/// Returns the entrypoint refusal or evaluation error.
#[cfg(test)]
pub(crate) fn dispatch_argv(
    lib: &WorldCommandLib,
    argv: &[String],
) -> std::result::Result<Datum, String> {
    let entry = WorldEntrypoint {
        product: lib.product.clone(),
        load: lib.load.clone(),
    };
    match entry.dispatch(argv).map_err(|error| error.to_string())? {
        Outcome::Value(value) => Ok(value),
        Outcome::Refused { call, reason } => Err(format!("refused {call}: {reason}")),
        Outcome::OverCeiling { command, .. } => Err(format!("over ceiling: {command}")),
    }
}
