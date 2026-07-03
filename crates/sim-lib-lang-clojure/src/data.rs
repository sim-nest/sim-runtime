use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, NumberLiteral, Result, Symbol, Value};
use sim_lib_namespace::{Namespace, NamespaceKind};
use sim_lib_sequence::{TransducerPipeline, persistent_list, sequence_for_profile, transduce};

use crate::{clojure_core_namespace_symbol, clojure_core_profile_symbol};

/// Reducer step used to fold sequence values during a Clojure-profile transduce.
///
/// Takes the running accumulator and the next value and returns the next accumulator.
pub type ClojureReducer =
    Arc<dyn Fn(&mut Cx, Value, Value) -> Result<Value> + Send + Sync + 'static>;

/// Realizes a decoded EDN [`Expr`] as a runtime [`Value`] using the sequence organ.
///
/// Collections become persistent list/vector/set/map values; scalars round-trip
/// through the factory. Map keys are coerced via the EDN key rules.
pub fn edn_expr_to_value(cx: &mut Cx, expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Nil => cx.factory().nil(),
        Expr::Bool(value) => cx.factory().bool(*value),
        Expr::Number(NumberLiteral { domain, canonical }) => cx
            .factory()
            .number_literal(domain.clone(), canonical.clone()),
        Expr::Symbol(symbol) => cx.factory().symbol(symbol.clone()),
        Expr::String(value) => cx.factory().string(value.clone()),
        Expr::Bytes(value) => cx.factory().bytes(value.clone()),
        Expr::List(items) => {
            let values = exprs_to_values(cx, items)?;
            sim_lib_sequence::persistent_list(cx, values)
        }
        Expr::Vector(items) => {
            let values = exprs_to_values(cx, items)?;
            sim_lib_sequence::persistent_vector(cx, values)
        }
        Expr::Set(items) => {
            let values = exprs_to_values(cx, items)?;
            sim_lib_sequence::persistent_set(cx, values)
        }
        Expr::Map(entries) => {
            let entries = entries
                .iter()
                .map(|(key, value)| Ok((map_key(key)?, edn_expr_to_value(cx, value)?)))
                .collect::<Result<Vec<_>>>()?;
            sim_lib_sequence::persistent_map(cx, entries)
        }
        other => cx.factory().expr(other.clone()),
    }
}

/// Builds a Clojure persistent vector value from the given items.
pub fn clojure_persistent_data(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    sim_lib_sequence::persistent_vector(cx, items)
}

/// Builds a sequence value tagged for the Clojure-core profile from the given items.
///
/// Wraps a persistent list as a sequence object bound to [`clojure_core_profile_symbol`].
pub fn clojure_profile_sequence(cx: &mut Cx, items: Vec<Value>) -> Result<Value> {
    let list = persistent_list(cx, items)?;
    let sequence = sim_lib_sequence::sequence_from_list_value(cx, list)?;
    sequence_for_profile(cx, clojure_core_profile_symbol(), sequence)
}

/// Runs a transducer pipeline over a source sequence using the sequence organ.
///
/// Thin Clojure-profile wrapper over [`sim_lib_sequence::transduce`] with a
/// [`ClojureReducer`] fold step.
pub fn clojure_transduce(
    cx: &mut Cx,
    source: &Value,
    pipeline: TransducerPipeline,
    init: Value,
    reducer: ClojureReducer,
) -> Result<Value> {
    transduce(cx, source, pipeline, init, reducer)
}

/// Builds the `clojure.core` namespace mapping surface names onto organ targets.
///
/// Maps `map`/`transduce` onto the sequence organ and `recur` onto the control
/// organ, exporting each.
pub fn clojure_core_namespace() -> Result<Namespace> {
    let mut namespace = Namespace::new(clojure_core_namespace_symbol(), NamespaceKind::Module);
    namespace.define(Symbol::new("map"), Symbol::qualified("sequence", "map.v1"))?;
    namespace.define(
        Symbol::new("transduce"),
        Symbol::qualified("sequence", "transduce.v1"),
    )?;
    namespace.define(
        Symbol::new("recur"),
        Symbol::qualified("control", "abort.v1"),
    )?;
    for exported in [
        Symbol::new("map"),
        Symbol::new("transduce"),
        Symbol::new("recur"),
    ] {
        namespace.export(exported)?;
    }
    Ok(namespace)
}

fn exprs_to_values(cx: &mut Cx, items: &[Expr]) -> Result<Vec<Value>> {
    items
        .iter()
        .map(|item| edn_expr_to_value(cx, item))
        .collect()
}

fn map_key(expr: &Expr) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        Expr::String(value) => Ok(Symbol::new(value.clone())),
        _ => Err(Error::TypeMismatch {
            expected: "EDN symbol, keyword, or string map key",
            found: expr_kind(expr),
        }),
    }
}

use sim_value::kind::expr_kind;
