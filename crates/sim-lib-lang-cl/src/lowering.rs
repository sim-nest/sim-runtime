use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

use sim_kernel::{Cx, Expr, LocatedExprTree, Origin, Result, Symbol};
use sim_lib_binding::{BindingProfileModes, HygieneMode};

use crate::{ClLiteRuntime, call_cl_value};

const SOURCE_ARG_ANNOTATION_NAMESPACE: &str = "cl";
const SOURCE_ARG_ANNOTATION_NAME: &str = "source-arg";
const EXPLICIT_HYGIENE_NAMESPACE: &str = "cl-hygiene";
const GENERATED_HYGIENE_NAMESPACE: &str = "cl-hygiene-generated";

static NEXT_HYGIENE_SYMBOL: AtomicU64 = AtomicU64::new(1);

/// A CL-lite source expansion paired with its original source origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedClLiteExpansion {
    /// Expanded source expression.
    pub expr: Expr,
    /// Source origin preserved from the decoded input tree.
    pub origin: Option<Origin>,
}

/// Symbol marker for identifiers that opt into hygiene under
/// [`HygieneMode::Explicit`].
pub fn cl_explicit_hygiene_symbol(name: &str) -> Symbol {
    Symbol::qualified(EXPLICIT_HYGIENE_NAMESPACE, name)
}

/// Expands one decoded CL-lite source tree through the runtime macro table.
///
/// Macro arguments are passed to macro functions as expression values carrying
/// a private source-argument marker so hygiene rewriting can preserve caller
/// syntax while renaming macro-introduced bindings.
pub fn expand_cl_lite_tree(
    cx: &mut Cx,
    runtime: &ClLiteRuntime,
    tree: &LocatedExprTree,
    modes: BindingProfileModes,
) -> Result<LocatedClLiteExpansion> {
    Ok(LocatedClLiteExpansion {
        expr: expand_cl_lite_expr(cx, runtime, &tree.expr, modes)?,
        origin: tree.origin.clone(),
    })
}

/// Expands one CL-lite source expression through the runtime macro table.
pub fn expand_cl_lite_expr(
    cx: &mut Cx,
    runtime: &ClLiteRuntime,
    expr: &Expr,
    modes: BindingProfileModes,
) -> Result<Expr> {
    expand_expr(cx, runtime, expr, modes)
}

fn expand_expr(
    cx: &mut Cx,
    runtime: &ClLiteRuntime,
    expr: &Expr,
    modes: BindingProfileModes,
) -> Result<Expr> {
    match expr {
        Expr::List(items) => expand_list(cx, runtime, items, modes),
        Expr::Call { operator, args } => Ok(Expr::Call {
            operator: Box::new(expand_expr(cx, runtime, operator, modes)?),
            args: args
                .iter()
                .map(|arg| expand_expr(cx, runtime, arg, modes))
                .collect::<Result<Vec<_>>>()?,
        }),
        Expr::Block(items) => Ok(Expr::Block(
            items
                .iter()
                .map(|item| expand_expr(cx, runtime, item, modes))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Vector(items) => Ok(Expr::Vector(
            items
                .iter()
                .map(|item| expand_expr(cx, runtime, item, modes))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Map(entries) => Ok(Expr::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        expand_expr(cx, runtime, key, modes)?,
                        expand_expr(cx, runtime, value, modes)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Set(items) => Ok(Expr::Set(
            items
                .iter()
                .map(|item| expand_expr(cx, runtime, item, modes))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Quote { .. } => Ok(expr.clone()),
        Expr::Annotated { expr, annotations } => Ok(Expr::Annotated {
            expr: Box::new(expand_expr(cx, runtime, expr, modes)?),
            annotations: annotations
                .iter()
                .map(|(name, value)| Ok((name.clone(), expand_expr(cx, runtime, value, modes)?)))
                .collect::<Result<Vec<_>>>()?,
        }),
        Expr::Extension { tag, payload } => Ok(Expr::Extension {
            tag: tag.clone(),
            payload: Box::new(expand_expr(cx, runtime, payload, modes)?),
        }),
        Expr::Prefix { operator, arg } => Ok(Expr::Prefix {
            operator: operator.clone(),
            arg: Box::new(expand_expr(cx, runtime, arg, modes)?),
        }),
        Expr::Postfix { operator, arg } => Ok(Expr::Postfix {
            operator: operator.clone(),
            arg: Box::new(expand_expr(cx, runtime, arg, modes)?),
        }),
        Expr::Infix {
            operator,
            left,
            right,
        } => Ok(Expr::Infix {
            operator: operator.clone(),
            left: Box::new(expand_expr(cx, runtime, left, modes)?),
            right: Box::new(expand_expr(cx, runtime, right, modes)?),
        }),
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Symbol(_)
        | Expr::Local(_)
        | Expr::String(_)
        | Expr::Bytes(_) => Ok(expr.clone()),
    }
}

fn expand_list(
    cx: &mut Cx,
    runtime: &ClLiteRuntime,
    items: &[Expr],
    modes: BindingProfileModes,
) -> Result<Expr> {
    if let Some(Expr::Symbol(head)) = items.first() {
        if head == &Symbol::new("quote") {
            return Ok(Expr::List(items.to_vec()));
        }
        if let Some(macro_value) = runtime.macro_function(head) {
            let args = items[1..]
                .iter()
                .map(|arg| cx.factory().expr(mark_source_arg(arg.clone())))
                .collect::<Result<Vec<_>>>()?;
            let expanded = call_cl_value(cx, &macro_value, args)?;
            let expanded_expr = expanded.object().as_expr(cx)?;
            let rewritten = rewrite_hygiene_expr(&expanded_expr, modes, &BTreeMap::new());
            return expand_expr(cx, runtime, &rewritten, modes);
        }
    }

    Ok(Expr::List(
        items
            .iter()
            .map(|item| expand_expr(cx, runtime, item, modes))
            .collect::<Result<Vec<_>>>()?,
    ))
}

fn mark_source_arg(expr: Expr) -> Expr {
    Expr::Annotated {
        expr: Box::new(expr),
        annotations: vec![(source_arg_annotation_symbol(), Expr::Bool(true))],
    }
}

fn source_arg_annotation_symbol() -> Symbol {
    Symbol::qualified(SOURCE_ARG_ANNOTATION_NAMESPACE, SOURCE_ARG_ANNOTATION_NAME)
}

fn rewrite_hygiene_expr(
    expr: &Expr,
    modes: BindingProfileModes,
    env: &BTreeMap<Symbol, Symbol>,
) -> Expr {
    match expr {
        Expr::Annotated { expr, annotations } if is_source_arg_annotation(annotations) => {
            (**expr).clone()
        }
        Expr::Symbol(symbol) => Expr::Symbol(
            env.get(symbol)
                .cloned()
                .unwrap_or_else(|| normalize_free_symbol(symbol)),
        ),
        Expr::List(items) if is_let_form(items) => rewrite_let_form(items, modes, env)
            .unwrap_or_else(|| rewrite_plain_list(items, modes, env)),
        Expr::List(items) => rewrite_plain_list(items, modes, env),
        Expr::Call { operator, args } => Expr::Call {
            operator: Box::new(rewrite_hygiene_expr(operator, modes, env)),
            args: args
                .iter()
                .map(|arg| rewrite_hygiene_expr(arg, modes, env))
                .collect(),
        },
        Expr::Block(items) => Expr::Block(
            items
                .iter()
                .map(|item| rewrite_hygiene_expr(item, modes, env))
                .collect(),
        ),
        Expr::Vector(items) => Expr::Vector(
            items
                .iter()
                .map(|item| rewrite_hygiene_expr(item, modes, env))
                .collect(),
        ),
        Expr::Map(entries) => Expr::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        rewrite_hygiene_expr(key, modes, env),
                        rewrite_hygiene_expr(value, modes, env),
                    )
                })
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(
            items
                .iter()
                .map(|item| rewrite_hygiene_expr(item, modes, env))
                .collect(),
        ),
        Expr::Quote { .. } => expr.clone(),
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(rewrite_hygiene_expr(expr, modes, env)),
            annotations: annotations
                .iter()
                .map(|(name, value)| (name.clone(), rewrite_hygiene_expr(value, modes, env)))
                .collect(),
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag: tag.clone(),
            payload: Box::new(rewrite_hygiene_expr(payload, modes, env)),
        },
        Expr::Prefix { operator, arg } => Expr::Prefix {
            operator: operator.clone(),
            arg: Box::new(rewrite_hygiene_expr(arg, modes, env)),
        },
        Expr::Postfix { operator, arg } => Expr::Postfix {
            operator: operator.clone(),
            arg: Box::new(rewrite_hygiene_expr(arg, modes, env)),
        },
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Infix {
            operator: operator.clone(),
            left: Box::new(rewrite_hygiene_expr(left, modes, env)),
            right: Box::new(rewrite_hygiene_expr(right, modes, env)),
        },
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Local(_)
        | Expr::String(_)
        | Expr::Bytes(_) => expr.clone(),
    }
}

fn rewrite_plain_list(
    items: &[Expr],
    modes: BindingProfileModes,
    env: &BTreeMap<Symbol, Symbol>,
) -> Expr {
    Expr::List(
        items
            .iter()
            .map(|item| rewrite_hygiene_expr(item, modes, env))
            .collect(),
    )
}

fn rewrite_let_form(
    items: &[Expr],
    modes: BindingProfileModes,
    env: &BTreeMap<Symbol, Symbol>,
) -> Option<Expr> {
    let Expr::List(bindings) = items.get(1)? else {
        return None;
    };

    let mut scoped_env = env.clone();
    let mut rewritten_bindings = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let Expr::List(pair) = binding else {
            return None;
        };
        let Some(Expr::Symbol(symbol)) = pair.first() else {
            return None;
        };
        let rewritten_symbol = rewrite_binding_symbol(symbol, modes);
        if rewritten_symbol != *symbol {
            scoped_env.insert(symbol.clone(), rewritten_symbol.clone());
        }
        let mut rewritten_pair = Vec::with_capacity(pair.len());
        rewritten_pair.push(Expr::Symbol(rewritten_symbol));
        rewritten_pair.extend(
            pair.iter()
                .skip(1)
                .map(|expr| rewrite_hygiene_expr(expr, modes, env)),
        );
        rewritten_bindings.push(Expr::List(rewritten_pair));
    }

    let mut rewritten_items = Vec::with_capacity(items.len());
    rewritten_items.push(items[0].clone());
    rewritten_items.push(Expr::List(rewritten_bindings));
    rewritten_items.extend(
        items[2..]
            .iter()
            .map(|expr| rewrite_hygiene_expr(expr, modes, &scoped_env)),
    );
    Some(Expr::List(rewritten_items))
}

fn rewrite_binding_symbol(symbol: &Symbol, modes: BindingProfileModes) -> Symbol {
    let plain = plain_symbol(symbol);
    match modes.hygiene {
        HygieneMode::Hygienic => fresh_hygienic_symbol(&plain),
        HygieneMode::Explicit if is_explicit_hygiene_symbol(symbol) => {
            fresh_hygienic_symbol(&plain)
        }
        HygieneMode::Explicit | HygieneMode::Unhygienic => plain,
    }
}

fn fresh_hygienic_symbol(base: &Symbol) -> Symbol {
    let id = NEXT_HYGIENE_SYMBOL.fetch_add(1, Ordering::Relaxed);
    Symbol::qualified(GENERATED_HYGIENE_NAMESPACE, format!("{}-{id}", base.name))
}

fn normalize_free_symbol(symbol: &Symbol) -> Symbol {
    plain_symbol(symbol)
}

fn plain_symbol(symbol: &Symbol) -> Symbol {
    if is_explicit_hygiene_symbol(symbol) {
        Symbol::new(symbol.name.to_string())
    } else {
        symbol.clone()
    }
}

fn is_explicit_hygiene_symbol(symbol: &Symbol) -> bool {
    symbol.namespace.as_deref() == Some(EXPLICIT_HYGIENE_NAMESPACE)
}

fn is_source_arg_annotation(annotations: &[(Symbol, Expr)]) -> bool {
    annotations.iter().any(|(name, value)| {
        name == &source_arg_annotation_symbol() && matches!(value, Expr::Bool(true))
    })
}

fn is_let_form(items: &[Expr]) -> bool {
    matches!(items.first(), Some(Expr::Symbol(symbol)) if symbol == &Symbol::new("let"))
}
