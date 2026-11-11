// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compose steps and datum rendering for the loaded world command.

use sim_kernel::{Cx, Datum, Error, Expr, Result, Symbol, Value};

use crate::command::ModelDomainLoad;
use crate::grant::{self, ModelDomainGrant};
use crate::{WorldError, WorldProduct};

pub(crate) fn eval_error(error: WorldError) -> Error {
    Error::Eval(error.to_string())
}

pub(crate) fn compose(
    product: &WorldProduct,
    grant: &mut ModelDomainGrant,
    args: &[String],
    usage: &str,
) -> Result<()> {
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
                let kind = step_arg(args, index + 1, usage)?;
                let fact = step_arg(args, index + 2, usage)?;
                let value = step_arg(args, index + 3, usage)?;
                emit_use(grant, "project", || {
                    product
                        .project(kind, fact, Datum::String(value.to_owned()), None)
                        .map(|projection| projection.value)
                })?;
                index += 4;
            }
            "diff" => {
                let kind = step_arg(args, index + 1, usage)?;
                let fact = step_arg(args, index + 2, usage)?;
                let before = step_arg(args, index + 3, usage)?;
                let after = step_arg(args, index + 4, usage)?;
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
                let conclusion = step_arg(args, index + 1, usage)?;
                let fact = step_arg(args, index + 2, usage)?;
                emit_use(grant, "why", || product.why(conclusion, fact))?;
                index += 3;
            }
            _ => return Err(Error::Eval(usage.to_owned())),
        }
    }
    Ok(())
}

fn step_arg<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Error::Eval(usage.to_owned()))
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

pub(crate) fn load_receipt(load: &ModelDomainLoad) -> Datum {
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

pub(crate) fn envelope_args(cx: &mut Cx, envelope: Option<&Value>) -> Result<Vec<String>> {
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

pub(crate) fn refusal(call: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "refusal-v1"),
        fields: vec![
            (Symbol::new("call"), Datum::String(call.to_owned())),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
        ],
    }
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
