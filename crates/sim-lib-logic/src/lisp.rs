use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Cx, Expr, Lib, LibManifest, LibTarget, Linker, Result, Symbol, Value, Version,
};

use crate::{
    capabilities::logic_db_write_capability,
    codec::{consult_expr, consult_path},
    error::logic_eval_error,
    lisp_runtime::{
        CONFIG_SYMBOL, DB_SYMBOL, LogicConfigState, LogicDbState, LogicFunction, config_value,
        keyword, logic_config_state, logic_db_state, query_config, string_expr, symbol_expr,
        unquote, usize_from_expr,
    },
    model::SearchStrategy,
    query::{query, query_all, query_bool, query_one},
    shapes::{register_logic_shapes, require_logic_stream},
};

const LOGIC_LIB_ID: &str = "logic";

/// The loadable logic organ: shapes, functions, and database/config state.
///
/// Implements the kernel `Lib` contract and installs the `logic/*` surface
/// (assert, retract, query, unify, and the logic shapes). Load it with
/// [`install_logic_lib`]; see the [`README`](https://docs.rs/sim-runtime).
pub struct LogicLib;

impl Lib for LogicLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::new(LOGIC_LIB_ID),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: logic_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        register_logic_shapes(linker, cx)?;
        register_logic_functions(cx, linker)?;
        linker.value(
            Symbol::qualified("logic", DB_SYMBOL),
            cx.factory().opaque(Arc::new(LogicDbState::default()))?,
        )?;
        linker.value(
            Symbol::qualified("logic", CONFIG_SYMBOL),
            cx.factory().opaque(Arc::new(LogicConfigState::default()))?,
        )?;
        Ok(())
    }
}

/// Installs the [`LogicLib`] into `cx`, idempotently.
///
/// Repeated calls are no-ops once the organ is loaded.
pub fn install_logic_lib(cx: &mut Cx) -> Result<()> {
    sim_lib_core::install_once(cx, &LogicLib).map(|_| ())
}

/// Resolves `goal` against the installed logic database and returns the result.
///
/// Installs the logic organ if needed, then resolves under the stored
/// [`LogicConfig`](crate::LogicConfig) (optionally overriding the answer limit
/// and stream buffer). When `stream` is true the result is a logic answer
/// stream object; otherwise it is the first answer as a value, or nil when the
/// goal fails.
pub fn realize_logic(
    cx: &mut Cx,
    goal: Expr,
    answer_limit: Option<usize>,
    stream_buffer: Option<usize>,
    stream: bool,
) -> Result<Value> {
    install_logic_lib(cx)?;
    let state = logic_config_state(cx)?;
    let mut config = state.lock()?.clone();
    if let Some(limit) = answer_limit {
        config.limits.max_answers = Some(limit);
    }
    if let Some(buffer) = stream_buffer {
        config.stream_buffer = buffer;
    }
    let db = logic_db_state(cx)?.lock()?.clone();
    if stream {
        let stream = query(cx, &db, &config, goal)?;
        return cx.factory().opaque(Arc::new(stream));
    }
    match query_one(cx, &db, &config, goal)? {
        Some(matched) => sim_kernel::shape_match_value(cx, matched),
        None => cx.factory().nil(),
    }
}

fn logic_exports() -> Vec<sim_kernel::Export> {
    let mut exports = vec![
        sim_kernel::Export::Value {
            symbol: Symbol::qualified("logic", DB_SYMBOL),
        },
        sim_kernel::Export::Value {
            symbol: Symbol::qualified("logic", CONFIG_SYMBOL),
        },
    ];
    for symbol in [
        Symbol::qualified("logic", "Var"),
        Symbol::qualified("logic", "Goal"),
        Symbol::qualified("logic", "Clause"),
        Symbol::qualified("logic", "Fact"),
        Symbol::qualified("logic", "Rule"),
        Symbol::qualified("logic", "Answer"),
        Symbol::qualified("logic", "Config"),
    ] {
        exports.push(sim_kernel::Export::Shape {
            symbol,
            shape_id: None,
        });
    }
    for symbol in [
        Symbol::qualified("logic", "config"),
        Symbol::qualified("logic", "assert!"),
        Symbol::qualified("logic", "retract!"),
        Symbol::qualified("logic", "facts"),
        Symbol::qualified("logic", "consult"),
        Symbol::qualified("logic", "consult!"),
        Symbol::qualified("logic", "stream-next"),
        Symbol::qualified("logic", "stream-close"),
        Symbol::qualified("logic", "query"),
        Symbol::qualified("logic", "query/one"),
        Symbol::qualified("logic", "query/all"),
        Symbol::qualified("logic", "query?"),
        Symbol::qualified("logic", "predicate?"),
    ] {
        exports.push(sim_kernel::Export::Function {
            symbol,
            function_id: None,
        });
    }
    exports
}

fn register_logic_functions(cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
    for (symbol, implementation) in [
        (
            Symbol::qualified("logic", "config"),
            logic_config_fn as fn(&mut Cx, &[Expr]) -> Result<Value>,
        ),
        (Symbol::qualified("logic", "assert!"), logic_assert_fn),
        (Symbol::qualified("logic", "retract!"), logic_retract_fn),
        (Symbol::qualified("logic", "facts"), logic_facts_fn),
        (Symbol::qualified("logic", "consult"), logic_consult_fn),
        (
            Symbol::qualified("logic", "consult!"),
            logic_consult_bang_fn,
        ),
        (
            Symbol::qualified("logic", "stream-next"),
            logic_stream_next_fn,
        ),
        (
            Symbol::qualified("logic", "stream-close"),
            logic_stream_close_fn,
        ),
        (Symbol::qualified("logic", "query"), logic_query_fn),
        (Symbol::qualified("logic", "query/one"), logic_query_one_fn),
        (Symbol::qualified("logic", "query/all"), logic_query_all_fn),
        (Symbol::qualified("logic", "query?"), logic_query_bool_fn),
        (Symbol::qualified("logic", "predicate?"), logic_predicate_fn),
    ] {
        linker.function_value(
            symbol.clone(),
            cx.factory().opaque(Arc::new(LogicFunction {
                symbol,
                implementation,
            }))?,
        )?;
    }
    Ok(())
}

fn logic_config_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let state = logic_config_state(cx)?;
    let mut config = state.lock()?.clone();
    if !args.len().is_multiple_of(2) {
        return Err(logic_eval_error(
            "logic/config options must be key/value pairs",
        ));
    }
    for pair in args.chunks(2) {
        let key = keyword(&pair[0])?;
        match key.as_str() {
            "max-depth" => config.limits.max_depth = usize_from_expr(cx, &pair[1])?,
            "stream-buffer" => config.stream_buffer = usize_from_expr(cx, &pair[1])?,
            "answer-limit" => config.limits.max_answers = Some(usize_from_expr(cx, &pair[1])?),
            "strategy" => {
                let symbol = symbol_expr(cx, &pair[1])?;
                config.strategy = SearchStrategy::from_symbol(&symbol)
                    .ok_or_else(|| logic_eval_error(format!("unsupported strategy {symbol}")))?;
            }
            other => {
                return Err(logic_eval_error(format!(
                    "logic/config does not support :{other}"
                )));
            }
        }
    }
    *state.lock()? = config.clone();
    config_value(cx, &config)
}

fn logic_assert_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(logic_eval_error("logic/assert! expects one quoted clause"));
    };
    let clause_expr = unquote(expr);
    logic_db_state(cx)?
        .lock()?
        .assert_clause_expr(clause_expr)?;
    cx.factory().bool(true)
}

fn logic_retract_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(logic_eval_error("logic/retract! expects one quoted clause"));
    };
    let removed = logic_db_state(cx)?
        .lock()?
        .retract_clause_expr(&unquote(expr))?;
    cx.factory().bool(removed)
}

fn logic_facts_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [expr] = args else {
        return Err(logic_eval_error("logic/facts expects one predicate symbol"));
    };
    let predicate = symbol_expr(cx, expr)?;
    let facts = logic_db_state(cx)?.lock()?.facts(&predicate);
    cx.factory().list(
        facts
            .into_iter()
            .map(|expr| cx.factory().expr(expr))
            .collect::<Result<Vec<_>>>()?,
    )
}

fn logic_consult_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [path_expr] = args else {
        return Err(logic_eval_error("logic/consult expects one path"));
    };
    let path = string_expr(cx, path_expr)?;
    let state = logic_db_state(cx)?;
    let mut db = state.lock()?;
    let count = consult_path(cx, &mut db, &path)?;
    drop(db);
    cx.factory().string(count.to_string())
}

fn logic_consult_bang_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    cx.require(&logic_db_write_capability())?;
    let [expr] = args else {
        return Err(logic_eval_error(
            "logic/consult! expects quoted clause data",
        ));
    };
    let state = logic_db_state(cx)?;
    let mut db = state.lock()?;
    let count = consult_expr(&mut db, unquote(expr))?;
    drop(db);
    cx.factory().string(count.to_string())
}

fn logic_query_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(logic_eval_error("query expects a goal"));
    };
    let config = query_config(cx, rest)?;
    let goal = unquote(goal);
    let db = logic_db_state(cx)?.lock()?.clone();
    let stream = query(cx, &db, &config, goal)?;
    cx.factory().opaque(Arc::new(stream))
}

fn logic_query_one_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(logic_eval_error("query/one expects a goal"));
    };
    let config = query_config(cx, rest)?;
    let db = logic_db_state(cx)?.lock()?.clone();
    match query_one(cx, &db, &config, unquote(goal))? {
        Some(matched) => sim_kernel::shape_match_value(cx, matched),
        None => cx.factory().nil(),
    }
}

fn logic_query_all_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(logic_eval_error("query/all expects a goal"));
    };
    let config = query_config(cx, rest)?;
    let db = logic_db_state(cx)?.lock()?.clone();
    let answers = query_all(cx, &db, &config, unquote(goal), config.limits.max_answers)?;
    let mut values = Vec::with_capacity(answers.len());
    for matched in answers {
        values.push(sim_kernel::shape_match_value(cx, matched)?);
    }
    cx.factory().list(values)
}

fn logic_query_bool_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [goal, rest @ ..] = args else {
        return Err(logic_eval_error("query? expects a goal"));
    };
    let config = query_config(cx, rest)?;
    let db = logic_db_state(cx)?.lock()?.clone();
    let accepted = query_bool(cx, &db, &config, unquote(goal))?;
    cx.factory().bool(accepted)
}

fn logic_predicate_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [expr] = args else {
        return Err(logic_eval_error("predicate? expects a predicate symbol"));
    };
    let predicate = symbol_expr(cx, expr)?;
    let exists = logic_db_state(cx)?.lock()?.predicate_exists(&predicate);
    cx.factory().bool(exists)
}

fn logic_stream_next_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [stream_expr] = args else {
        return Err(logic_eval_error("logic/stream-next expects a stream"));
    };
    let stream = cx.eval_expr(stream_expr.clone())?;
    match sim_kernel::Stream::next(require_logic_stream(&stream)?, cx)? {
        Some(value) => Ok(value),
        None => cx.factory().nil(),
    }
}

fn logic_stream_close_fn(cx: &mut Cx, args: &[Expr]) -> Result<Value> {
    let [stream_expr] = args else {
        return Err(logic_eval_error("logic/stream-close expects a stream"));
    };
    let stream = cx.eval_expr(stream_expr.clone())?;
    sim_kernel::Stream::close(require_logic_stream(&stream)?, cx)?;
    cx.factory().nil()
}
