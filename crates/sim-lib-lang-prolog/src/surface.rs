use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{
    AbiVersion, Args, Callable, ClassRef, Cx, Error, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, QuoteMode, RawArgs, ReadPolicy, Result, Symbol, Value, Version,
};
use sim_lib_logic::{
    LogicConfig, LogicDb, LogicPolicy, SearchStrategy, logic_consult_file_capability,
    logic_db_write_capability, query,
};

use crate::exports::prolog_export_declarations;

const PROLOG_LIB_ID: &str = "prolog";
const DB_SYMBOL: &str = "db";
const CONFIG_SYMBOL: &str = "config-state";

/// The loadable Prolog surface organ.
///
/// Loading the organ registers `prolog/*` functions plus the state handles used
/// by [`install_prolog_lib`] to point the active logic eval policy at the same
/// clause database.
pub struct PrologLib;

impl Lib for PrologLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::new(PROLOG_LIB_ID),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: prolog_export_declarations(),
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        register_prolog_functions(cx, linker)?;
        linker.value(
            Symbol::qualified("prolog", DB_SYMBOL),
            cx.factory().opaque(Arc::new(PrologDbState::default()))?,
        )?;
        linker.value(
            Symbol::qualified("prolog", CONFIG_SYMBOL),
            cx.factory()
                .opaque(Arc::new(PrologConfigState::default()))?,
        )?;
        Ok(())
    }
}

/// Installs the Prolog surface into `cx` and makes its logic policy active.
///
/// Repeated calls keep the installed database and reset the active eval policy
/// to the installed Prolog database, so direct expression evaluation and
/// `prolog/*` calls see the same asserted clauses.
pub fn install_prolog_lib(cx: &mut Cx) -> Result<()> {
    let _ = sim_lib_core::install_once(cx, &PrologLib)?;
    let db = prolog_db_state(cx)?.handle();
    let config = prolog_config_state(cx)?.lock()?.clone();
    cx.set_eval_policy(Arc::new(LogicPolicy::from_shared(db, config)));
    Ok(())
}

#[derive(Clone, Default)]
struct PrologDbState {
    inner: Arc<Mutex<LogicDb>>,
}

impl PrologDbState {
    fn handle(&self) -> Arc<Mutex<LogicDb>> {
        Arc::clone(&self.inner)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, LogicDb>> {
        self.inner
            .lock()
            .map_err(|_| Error::PoisonedLock("prolog db"))
    }
}

impl Object for PrologDbState {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<prolog-db>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PrologDbState {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("prolog", "DbState"),
        )
    }
}

#[derive(Clone, Default)]
struct PrologConfigState {
    inner: Arc<Mutex<LogicConfig>>,
}

impl PrologConfigState {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, LogicConfig>> {
        self.inner
            .lock()
            .map_err(|_| Error::PoisonedLock("prolog config"))
    }
}

impl Object for PrologConfigState {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<prolog-config>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PrologConfigState {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("prolog", "ConfigState"),
        )
    }
}

struct PrologFunction {
    symbol: Symbol,
    implementation: fn(&mut Cx, &[Expr]) -> Result<Value>,
}

impl Object for PrologFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PrologFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for PrologFunction {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(prolog_eval_error(format!(
            "{} must be called from source expressions",
            self.symbol
        )))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        (self.implementation)(cx, args.exprs())
    }
}

fn register_prolog_functions(cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
    for (symbol, implementation) in [
        (
            Symbol::qualified("prolog", "assert!"),
            prolog_assert_fn as fn(&mut Cx, &[Expr]) -> Result<Value>,
        ),
        (Symbol::qualified("prolog", "retract!"), prolog_retract_fn),
        (Symbol::qualified("prolog", "query"), prolog_query_fn),
        (
            Symbol::qualified("prolog", "query/all"),
            prolog_query_all_fn,
        ),
        (
            Symbol::qualified("prolog", "query-seq"),
            prolog_query_seq_fn,
        ),
        (Symbol::qualified("prolog", "consult"), prolog_consult_fn),
    ] {
        linker.function_value(
            symbol.clone(),
            cx.factory().opaque(Arc::new(PrologFunction {
                symbol,
                implementation,
            }))?,
        )?;
    }
    Ok(())
}

fn prolog_assert_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(prolog_eval_error(
            "prolog/assert! expects one quoted clause",
        ));
    };
    prolog_db_state(cx)?
        .lock()?
        .assert_clause_expr(unquote(expr))?;
    cx.factory().bool(true)
}

fn prolog_retract_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(prolog_eval_error(
            "prolog/retract! expects one quoted clause",
        ));
    };
    let removed = prolog_db_state(cx)?
        .lock()?
        .retract_clause_expr(&unquote(expr))?;
    cx.factory().bool(removed)
}

fn prolog_query_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(prolog_eval_error("prolog/query expects a goal"));
    };
    let config = prolog_query_config(cx, rest)?;
    let db = prolog_db_state(cx)?.lock()?.clone();
    let stream = query(cx, &db, &config, unquote(goal))?;
    match stream.collect(cx, Some(1))?.into_iter().next() {
        Some(answer) => Ok(answer),
        None => cx.factory().nil(),
    }
}

fn prolog_query_all_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(prolog_eval_error("prolog/query/all expects a goal"));
    };
    let config = prolog_query_config(cx, rest)?;
    let db = prolog_db_state(cx)?.lock()?.clone();
    let stream = query(cx, &db, &config, unquote(goal))?;
    let answers = stream.collect(cx, config.limits.max_answers)?;
    cx.factory().list(answers)
}

fn prolog_query_seq_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(prolog_eval_error("prolog/query-seq expects a goal"));
    };
    let config = prolog_query_config(cx, rest)?;
    let db = prolog_db_state(cx)?.lock()?.clone();
    let stream = query(cx, &db, &config, unquote(goal))?;
    cx.factory().opaque(Arc::new(stream))
}

fn prolog_consult_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(prolog_eval_error(
            "prolog/consult expects one path or quoted program",
        ));
    };
    let program = unquote(expr);
    let count = match program {
        Expr::String(path) => {
            let state = prolog_db_state(cx)?;
            let mut db = state.lock()?;
            prolog_consult_path(cx, &mut db, &path)?
        }
        Expr::Symbol(path) => {
            let state = prolog_db_state(cx)?;
            let mut db = state.lock()?;
            prolog_consult_path(cx, &mut db, &path.to_string())?
        }
        other => {
            let state = prolog_db_state(cx)?;
            let mut db = state.lock()?;
            prolog_consult_expr(&mut db, other)?
        }
    };
    cx.factory().string(count.to_string())
}

fn prolog_db_state(cx: &mut Cx) -> Result<PrologDbState> {
    cx.resolve_value(&Symbol::qualified("prolog", DB_SYMBOL))?
        .object()
        .downcast_ref::<PrologDbState>()
        .cloned()
        .ok_or(Error::TypeMismatch {
            expected: "prolog db state",
            found: "non-prolog-db",
        })
}

fn prolog_config_state(cx: &mut Cx) -> Result<PrologConfigState> {
    cx.resolve_value(&Symbol::qualified("prolog", CONFIG_SYMBOL))?
        .object()
        .downcast_ref::<PrologConfigState>()
        .cloned()
        .ok_or(Error::TypeMismatch {
            expected: "prolog config state",
            found: "non-prolog-config",
        })
}

fn prolog_query_config(cx: &mut Cx, options: &[Expr]) -> Result<LogicConfig> {
    let mut config = prolog_config_state(cx)?.lock()?.clone();
    if !options.len().is_multiple_of(2) {
        return Err(prolog_eval_error(
            "prolog query options must be key/value pairs",
        ));
    }
    for pair in options.chunks(2) {
        let key = keyword(&pair[0])?;
        match key.as_str() {
            "limit" | "answer-limit" | "max-answers" => {
                config.limits.max_answers = Some(usize_from_expr(cx, &pair[1])?)
            }
            "buffer" | "stream-buffer" => config.stream_buffer = usize_from_expr(cx, &pair[1])?,
            "strategy" => {
                let symbol = symbol_expr(cx, &pair[1])?;
                config.strategy = SearchStrategy::from_symbol(&symbol)
                    .ok_or_else(|| prolog_eval_error(format!("unsupported strategy {symbol}")))?;
            }
            other => {
                return Err(prolog_eval_error(format!(
                    "prolog query does not support :{other}"
                )));
            }
        }
    }
    Ok(config)
}

fn keyword(expr: &Expr) -> Result<String> {
    let Expr::Symbol(symbol) = expr else {
        return Err(prolog_eval_error("expected keyword symbol"));
    };
    Ok(symbol.name.trim_start_matches(':').to_owned())
}

fn symbol_expr(cx: &mut Cx, expr: &Expr) -> Result<Symbol> {
    match unquote(expr) {
        Expr::Symbol(symbol) => Ok(symbol),
        other => match cx.eval_expr(other)?.object().as_expr(cx)? {
            Expr::Symbol(symbol) => Ok(symbol),
            _ => Err(prolog_eval_error("expected symbol")),
        },
    }
}

fn usize_from_expr(cx: &mut Cx, expr: &Expr) -> Result<usize> {
    match unquote(expr) {
        Expr::Number(number) => number
            .canonical
            .parse::<usize>()
            .map_err(|_| prolog_eval_error(format!("expected usize, found {}", number.canonical))),
        Expr::String(text) => text
            .parse::<usize>()
            .map_err(|_| prolog_eval_error(format!("expected usize, found {text}"))),
        Expr::Symbol(symbol) => symbol
            .name
            .parse::<usize>()
            .map_err(|_| prolog_eval_error(format!("expected usize, found {symbol}"))),
        other => match cx.eval_expr(other)?.object().as_expr(cx)? {
            Expr::Number(number) => number.canonical.parse::<usize>().map_err(|_| {
                prolog_eval_error(format!("expected usize, found {}", number.canonical))
            }),
            Expr::String(text) => text
                .parse::<usize>()
                .map_err(|_| prolog_eval_error(format!("expected usize, found {text}"))),
            _ => Err(prolog_eval_error("expected usize value")),
        },
    }
}

fn unquote(expr: &Expr) -> Expr {
    match expr {
        Expr::Quote {
            mode: QuoteMode::Quote,
            expr,
        } => (**expr).clone(),
        other => other.clone(),
    }
}

fn prolog_consult_path(cx: &mut Cx, db: &mut LogicDb, path: &str) -> Result<usize> {
    cx.require(&logic_consult_file_capability())?;
    let bytes = fs::read(path).map_err(|err| prolog_eval_error(err.to_string()))?;
    let codec = codec_for_path(path);
    let expr = decode_with_codec(
        cx,
        &codec,
        match codec.name.as_ref() {
            "binary" | "binary-base64" => Input::Bytes(bytes),
            _ => Input::Text(
                String::from_utf8(bytes).map_err(|err| prolog_eval_error(err.to_string()))?,
            ),
        },
        ReadPolicy::default(),
    )?;
    prolog_consult_expr(db, expr)
}

fn prolog_consult_expr(db: &mut LogicDb, expr: Expr) -> Result<usize> {
    match expr {
        Expr::List(items) => {
            let mut count = 0usize;
            for item in items {
                db.assert_clause_expr(item)?;
                count += 1;
            }
            Ok(count)
        }
        other => {
            db.assert_clause_expr(other)?;
            Ok(1)
        }
    }
}

fn codec_for_path(path: &str) -> Symbol {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
    {
        "simlogicb64" | "simb64" => Symbol::qualified("codec", "binary-base64"),
        "json" => Symbol::qualified("codec", "json"),
        "alg" => Symbol::qualified("codec", "algol"),
        "slb8" => Symbol::qualified("codec", "binary"),
        _ => Symbol::qualified("codec", "lisp"),
    }
}

fn prolog_eval_error(message: impl Into<String>) -> Error {
    Error::Eval(message.into())
}
