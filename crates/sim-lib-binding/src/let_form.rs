//! The `let` binding special form.
//!
//! `let` is a COOKBOOK_7 Category B eval-policy organ: a special form that
//! introduces lexical bindings. Like [`crate`]'s other binding machinery it is
//! lexical and parallel -- every initializer is evaluated in the OUTER scope, so
//! no binding can observe another, and the body then runs in a fresh child scope.
//!
//! It is implemented with the SIM-native special-form mechanism: a [`Callable`]
//! that overrides [`Callable::call_exprs`] to take its argument expressions
//! UNEVALUATED, so it can decide what to evaluate and in which scope. The
//! bindings are installed into the kernel [`Env`] (via [`Cx::with_env`]) that the
//! evaluator consults for symbol lookup, so a bound name resolves in the body and
//! is gone again once the form returns.

use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Env, Error, Expr, Object, ObjectCompat, RawArgs, Result, Symbol,
    Value,
};

/// The `let` special form: `(let ((name init)...) body...)`.
#[derive(Clone, Copy)]
pub struct LetForm;

impl LetForm {
    /// The bare `let` symbol this form registers under.
    pub fn symbol() -> Symbol {
        Symbol::new("let")
    }
}

/// Parse one binding clause into `(name, init)`.
///
/// A clause decodes either as a call `(name init)` (operator + one argument) or
/// as a two-element data list `[name init]`, depending on the codec's lowering;
/// both are accepted so the form works from a lisp surface and from a data codec.
fn parse_clause(clause: Expr) -> Result<(Symbol, Expr)> {
    let bad = || Error::Eval("let binding must be (name init)".to_owned());
    match clause {
        Expr::Call { operator, args } => {
            let Expr::Symbol(name) = *operator else {
                return Err(bad());
            };
            let mut args = args.into_iter();
            let (Some(init), None) = (args.next(), args.next()) else {
                return Err(bad());
            };
            Ok((name, init))
        }
        Expr::List(items) | Expr::Vector(items) => {
            let mut items = items.into_iter();
            match (items.next(), items.next(), items.next()) {
                (Some(Expr::Symbol(name)), Some(init), None) => Ok((name, init)),
                _ => Err(bad()),
            }
        }
        _ => Err(bad()),
    }
}

/// Parse the bindings container (a list/vector of clauses) into `(name, init)`s.
fn parse_bindings(bindings: Expr) -> Result<Vec<(Symbol, Expr)>> {
    let clauses = match bindings {
        Expr::List(items) | Expr::Vector(items) => items,
        Expr::Nil => Vec::new(),
        _ => return Err(Error::Eval("let bindings must be a list".to_owned())),
    };
    clauses.into_iter().map(parse_clause).collect()
}

impl Object for LetForm {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<special-form let>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LetForm {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LetForm {
    /// `let` cannot run on pre-evaluated arguments: its first argument is the
    /// binding form, which must stay unevaluated. Calling it eagerly is a usage
    /// error; the real semantics live in [`Self::call_exprs`].
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(Error::Eval(
            "let is a special form and cannot be applied to evaluated arguments".to_owned(),
        ))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let mut exprs = args.into_exprs().into_iter();
        let Some(bindings) = exprs.next() else {
            return Err(Error::Eval(
                "let expects (let (bindings...) body...)".to_owned(),
            ));
        };
        let body: Vec<Expr> = exprs.collect();

        // Parallel bindings: evaluate every initializer in the OUTER scope first.
        let clauses = parse_bindings(bindings)?;
        let mut bound = Vec::with_capacity(clauses.len());
        for (name, init) in clauses {
            let value = cx.eval_expr(init)?;
            bound.push((name, value));
        }

        // Install the bindings in a fresh child scope and run the body there.
        let child = Env::child(Arc::new(cx.env().clone()));
        cx.with_env(child, |cx| {
            for (name, value) in bound {
                cx.env_mut().define(name, value);
            }
            let mut last = cx.factory().nil()?;
            for expr in body {
                last = cx.eval_expr(expr)?;
            }
            Ok(last)
        })
    }
}
