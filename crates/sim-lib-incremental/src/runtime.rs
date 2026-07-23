//! Runtime library wiring and exported callables.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, ClassRef, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, Object, ObjectCompat, RawArgs, Result, ShapeRef, Symbol, Value, Version,
};

use crate::{
    cap::{
        incremental_read_capability, incremental_verify_capability, incremental_write_capability,
    },
    claims::publish_incremental_organ_claims_for_lib,
    model::{incremental_engine_value, require_incremental_engine},
    shapes::{
        args_shape, engine_shape, key_shape, query_expr_shape, register_incremental_shapes,
        report_shape, result_shape,
    },
};

const INCREMENTAL_LIB_ID: &str = "sim/incremental";

/// The loadable incremental query organ.
pub struct IncrementalLib;

impl Lib for IncrementalLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::new(INCREMENTAL_LIB_ID),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: incremental_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        register_incremental_shapes(linker, cx)?;
        for kind in IncrementalFunctionKind::ALL {
            let function = IncrementalFunction { kind };
            linker.function_value(function.symbol(), cx.factory().opaque(Arc::new(function))?)?;
        }
        Ok(())
    }
}

/// Installs the incremental query organ into `cx`, idempotently.
pub fn install_incremental_lib(cx: &mut Cx) -> Result<()> {
    if let Some(lib_id) = sim_lib_core::install_once_id(cx, &IncrementalLib)? {
        publish_incremental_organ_claims_for_lib(cx, lib_id)?;
    }
    Ok(())
}

/// Export records produced by [`IncrementalLib`].
pub fn incremental_exports() -> Vec<Export> {
    let mut exports = Vec::new();
    for symbol in [
        Symbol::qualified("incremental", "Engine"),
        Symbol::qualified("incremental", "Key"),
        Symbol::qualified("incremental", "QueryExpr"),
        Symbol::qualified("incremental", "Report"),
    ] {
        exports.push(Export::Shape {
            symbol,
            shape_id: None,
        });
    }
    for kind in IncrementalFunctionKind::ALL {
        exports.push(Export::Function {
            symbol: kind.symbol(),
            function_id: None,
        });
    }
    exports
}

#[derive(Clone)]
struct IncrementalFunction {
    kind: IncrementalFunctionKind,
}

#[derive(Clone, Copy)]
enum IncrementalFunctionKind {
    Engine,
    Register,
    Invalidate,
    Verify,
    Explain,
    Snapshot,
    Metrics,
}

impl IncrementalFunctionKind {
    const ALL: [Self; 7] = [
        Self::Engine,
        Self::Register,
        Self::Invalidate,
        Self::Verify,
        Self::Explain,
        Self::Snapshot,
        Self::Metrics,
    ];

    fn symbol(self) -> Symbol {
        match self {
            Self::Engine => Symbol::qualified("incremental", "engine"),
            Self::Register => Symbol::qualified("incremental", "register"),
            Self::Invalidate => Symbol::qualified("incremental", "invalidate"),
            Self::Verify => Symbol::qualified("incremental", "verify"),
            Self::Explain => Symbol::qualified("incremental", "explain"),
            Self::Snapshot => Symbol::qualified("incremental", "snapshot"),
            Self::Metrics => Symbol::qualified("incremental", "metrics"),
        }
    }

    fn call(self, cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
        match self {
            Self::Engine => {
                expect_arity("incremental/engine", &values, 0)?;
                incremental_engine_value(cx)
            }
            Self::Register => register_value(cx, values),
            Self::Invalidate => {
                cx.require(&incremental_write_capability())?;
                let [engine, key] = expect_array("incremental/invalidate", values)?;
                let key = string_key(cx, &key)?;
                require_incremental_engine(&engine)?.invalidate(&key);
                cx.factory().bool(true)
            }
            Self::Verify => {
                cx.require(&incremental_verify_capability())?;
                let [engine, key] = expect_array("incremental/verify", values)?;
                let key = string_key(cx, &key)?;
                let expr = require_incremental_engine(&engine)?.verify(&key)?;
                cx.factory().expr(expr)
            }
            Self::Explain => {
                cx.require(&incremental_read_capability())?;
                let [engine, key] = expect_array("incremental/explain", values)?;
                let key = string_key(cx, &key)?;
                require_incremental_engine(&engine)?.explain(cx, &key)
            }
            Self::Snapshot => {
                cx.require(&incremental_read_capability())?;
                let [engine, key] = expect_array("incremental/snapshot", values)?;
                let key = string_key(cx, &key)?;
                require_incremental_engine(&engine)?.snapshot(cx, &key)
            }
            Self::Metrics => {
                cx.require(&incremental_read_capability())?;
                let [engine] = expect_array("incremental/metrics", values)?;
                require_incremental_engine(&engine)?.metrics(cx)
            }
        }
    }

    fn args_shape(self) -> ShapeRef {
        let symbol = Symbol::qualified(self.symbol().to_string(), "args");
        match self {
            Self::Engine => args_shape(symbol, Vec::new()),
            Self::Register => args_shape(
                symbol,
                vec![engine_shape(), key_shape(), query_expr_shape()],
            ),
            Self::Invalidate | Self::Verify | Self::Explain | Self::Snapshot => {
                args_shape(symbol, vec![engine_shape(), key_shape()])
            }
            Self::Metrics => args_shape(symbol, vec![engine_shape()]),
        }
    }

    fn result_shape(self) -> ShapeRef {
        let symbol = Symbol::qualified(self.symbol().to_string(), "result");
        match self {
            Self::Engine => result_shape(symbol),
            Self::Register | Self::Invalidate => result_shape(symbol),
            Self::Verify => result_shape(symbol),
            Self::Explain | Self::Snapshot | Self::Metrics => {
                sim_shape::shape_value(symbol, report_shape())
            }
        }
    }
}

impl Object for IncrementalFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.symbol()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IncrementalFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for IncrementalFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.kind.call(cx, args.into_vec())
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.kind.args_shape()))
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.kind.result_shape()))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        if matches!(self.kind, IncrementalFunctionKind::Register) {
            let [engine_expr, key_expr, source] = expect_expr_array("incremental/register", args)?;
            let engine = cx.eval_expr(engine_expr)?;
            let key = cx.eval_expr(key_expr)?;
            return register_expr(cx, engine, key, source);
        }
        let values = args
            .into_exprs()
            .into_iter()
            .map(|expr| cx.eval_expr(expr))
            .collect::<Result<Vec<_>>>()?;
        self.kind.call(cx, values)
    }
}

impl IncrementalFunction {
    fn symbol(&self) -> Symbol {
        self.kind.symbol()
    }
}

fn register_value(cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
    cx.require(&incremental_write_capability())?;
    let [engine, key, source] = expect_array("incremental/register", values)?;
    let key = string_key(cx, &key)?;
    let source = source.object().as_expr(cx)?;
    require_incremental_engine(&engine)?.register(key, source)?;
    cx.factory().bool(true)
}

fn register_expr(cx: &mut Cx, engine: Value, key: Value, source: Expr) -> Result<Value> {
    cx.require(&incremental_write_capability())?;
    let key = string_key(cx, &key)?;
    require_incremental_engine(&engine)?.register(key, source)?;
    cx.factory().bool(true)
}

fn string_key(cx: &mut Cx, value: &Value) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(key) => Ok(key),
        other => Err(Error::TypeMismatch {
            expected: "string key",
            found: expr_kind_name(&other),
        }),
    }
}

fn expect_arity(name: &'static str, values: &[Value], expected: usize) -> Result<()> {
    if values.len() == expected {
        Ok(())
    } else {
        Err(Error::Eval(format!(
            "{name} expects {expected} arguments, got {}",
            values.len()
        )))
    }
}

fn expect_array<const N: usize>(name: &'static str, values: Vec<Value>) -> Result<[Value; N]> {
    values.try_into().map_err(|values: Vec<Value>| {
        Error::Eval(format!(
            "{name} expects {N} arguments, got {}",
            values.len()
        ))
    })
}

fn expect_expr_array<const N: usize>(name: &'static str, args: RawArgs) -> Result<[Expr; N]> {
    args.into_exprs().try_into().map_err(|exprs: Vec<Expr>| {
        Error::Eval(format!(
            "{name} expects {N} expressions, got {}",
            exprs.len()
        ))
    })
}

fn expr_kind_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Nil => "nil",
        Expr::Bool(_) => "bool",
        Expr::Number(_) => "number",
        Expr::Symbol(_) => "symbol",
        Expr::Local(_) => "local",
        Expr::String(_) => "string",
        Expr::Bytes(_) => "bytes",
        Expr::List(_) => "list",
        Expr::Vector(_) => "vector",
        Expr::Map(_) => "map",
        Expr::Set(_) => "set",
        Expr::Call { .. } => "call",
        Expr::Infix { .. } => "infix",
        Expr::Prefix { .. } => "prefix",
        Expr::Postfix { .. } => "postfix",
        Expr::Block(_) => "block",
        Expr::Quote { .. } => "quote",
        Expr::Annotated { .. } => "annotated",
        Expr::Extension { .. } => "extension",
    }
}
