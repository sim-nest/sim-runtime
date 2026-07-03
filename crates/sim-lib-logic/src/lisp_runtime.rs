use std::sync::{Arc, Mutex};

use sim_kernel::{Args, Callable, ClassRef, Cx, Expr, Object, RawArgs, Result, Symbol, Value};

use crate::{LogicConfig, LogicDb, error::logic_eval_error, model::SearchStrategy};

pub(crate) const DB_SYMBOL: &str = "db";
pub(crate) const CONFIG_SYMBOL: &str = "config-state";

#[sim_citizen_derive::non_citizen(
    reason = "live logic database state; reconstruct from asserted facts and rules",
    kind = "handle",
    descriptor = "logic/Db"
)]
#[derive(Clone, Default)]
pub(crate) struct LogicDbState {
    inner: Arc<Mutex<LogicDb>>,
}

#[sim_citizen_derive::non_citizen(
    reason = "live logic configuration state; reconstruct from logic query configuration data",
    kind = "handle",
    descriptor = "logic/Config"
)]
#[derive(Clone, Default)]
pub(crate) struct LogicConfigState {
    inner: Arc<Mutex<LogicConfig>>,
}

impl LogicDbState {
    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, LogicDb>> {
        self.inner
            .lock()
            .map_err(|_| sim_kernel::Error::PoisonedLock("logic db"))
    }
}

impl LogicConfigState {
    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, LogicConfig>> {
        self.inner
            .lock()
            .map_err(|_| sim_kernel::Error::PoisonedLock("logic config"))
    }
}

impl Object for LogicDbState {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<logic-db>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for LogicDbState {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("logic", "DbState"),
        )
    }
}

impl Object for LogicConfigState {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<logic-config>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for LogicConfigState {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("logic", "ConfigState"),
        )
    }
}

pub(crate) struct LogicFunction {
    pub(crate) symbol: Symbol,
    pub(crate) implementation: fn(&mut Cx, &[Expr]) -> Result<Value>,
}

impl Object for LogicFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for LogicFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LogicFunction {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(logic_eval_error(format!(
            "{} must be called from source expressions",
            self.symbol
        )))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        (self.implementation)(cx, args.exprs())
    }
}

pub(crate) fn logic_db_state(cx: &mut Cx) -> Result<LogicDbState> {
    cx.resolve_value(&Symbol::qualified("logic", DB_SYMBOL))?
        .object()
        .downcast_ref::<LogicDbState>()
        .cloned()
        .ok_or(sim_kernel::Error::TypeMismatch {
            expected: "logic db state",
            found: "non-logic-db",
        })
}

pub(crate) fn logic_config_state(cx: &mut Cx) -> Result<LogicConfigState> {
    cx.resolve_value(&Symbol::qualified("logic", CONFIG_SYMBOL))?
        .object()
        .downcast_ref::<LogicConfigState>()
        .cloned()
        .ok_or(sim_kernel::Error::TypeMismatch {
            expected: "logic config state",
            found: "non-logic-config",
        })
}

pub(crate) fn config_value(cx: &mut Cx, config: &LogicConfig) -> Result<Value> {
    cx.factory().table(vec![
        (
            Symbol::new("max-depth"),
            cx.factory().string(config.limits.max_depth.to_string())?,
        ),
        (
            Symbol::new("stream-buffer"),
            cx.factory().string(config.stream_buffer.to_string())?,
        ),
        (
            Symbol::new("answer-limit"),
            match config.limits.max_answers {
                Some(limit) => cx.factory().string(limit.to_string())?,
                None => cx.factory().nil()?,
            },
        ),
        (
            Symbol::new("strategy"),
            cx.factory().symbol(config.strategy.as_symbol())?,
        ),
    ])
}

pub(crate) fn query_config(cx: &mut Cx, options: &[Expr]) -> Result<LogicConfig> {
    let state = logic_config_state(cx)?;
    let mut config = state.lock()?.clone();
    if !options.len().is_multiple_of(2) {
        return Err(logic_eval_error("query options must be key/value pairs"));
    }
    for pair in options.chunks(2) {
        let key = keyword(&pair[0])?;
        match key.as_str() {
            "limit" => config.limits.max_answers = Some(usize_from_expr(cx, &pair[1])?),
            "buffer" => config.stream_buffer = usize_from_expr(cx, &pair[1])?,
            "strategy" => {
                let symbol = symbol_expr(cx, &pair[1])?;
                config.strategy = SearchStrategy::from_symbol(&symbol)
                    .ok_or_else(|| logic_eval_error(format!("unsupported strategy {symbol}")))?;
            }
            other => return Err(logic_eval_error(format!("query does not support :{other}"))),
        }
    }
    Ok(config)
}

pub(crate) fn keyword(expr: &Expr) -> Result<String> {
    let Expr::Symbol(symbol) = expr else {
        return Err(logic_eval_error("expected keyword symbol"));
    };
    Ok(symbol.name.trim_start_matches(':').to_owned())
}

pub(crate) fn symbol_expr(cx: &mut Cx, expr: &Expr) -> Result<Symbol> {
    match unquote(expr) {
        Expr::Symbol(symbol) => Ok(symbol),
        other => match cx.eval_expr(other)?.object().as_expr(cx)? {
            Expr::Symbol(symbol) => Ok(symbol),
            _ => Err(logic_eval_error("expected symbol")),
        },
    }
}

pub(crate) fn string_expr(cx: &mut Cx, expr: &Expr) -> Result<String> {
    match unquote(expr) {
        Expr::String(text) => Ok(text),
        Expr::Symbol(symbol) => Ok(symbol.to_string()),
        other => match cx.eval_expr(other)?.object().as_expr(cx)? {
            Expr::String(text) => Ok(text),
            Expr::Symbol(symbol) => Ok(symbol.to_string()),
            _ => Err(logic_eval_error("expected string-like value")),
        },
    }
}

pub(crate) fn usize_from_expr(cx: &mut Cx, expr: &Expr) -> Result<usize> {
    let expr = unquote(expr);
    match expr {
        Expr::Number(number) => number
            .canonical
            .parse::<usize>()
            .map_err(|_| logic_eval_error(format!("expected usize, found {}", number.canonical))),
        other => {
            let value = cx.eval_expr(other)?;
            match value.object().as_expr(cx)? {
                Expr::Number(number) => number.canonical.parse::<usize>().map_err(|_| {
                    logic_eval_error(format!("expected usize, found {}", number.canonical))
                }),
                Expr::String(text) => text
                    .parse::<usize>()
                    .map_err(|_| logic_eval_error(format!("expected usize, found {text}"))),
                _ => Err(logic_eval_error("expected usize value")),
            }
        }
    }
}

pub(crate) fn unquote(expr: &Expr) -> Expr {
    match expr {
        Expr::Quote {
            mode: sim_kernel::QuoteMode::Quote,
            expr,
        } => (**expr).clone(),
        other => other.clone(),
    }
}
