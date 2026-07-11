//! The `if` conditional special form.
//!
//! `if` is the first eval-policy organ of COOKBOOK_7 Category B: a special form,
//! not an ordinary function. An ordinary function evaluates every argument before
//! it runs, which would evaluate BOTH branches of a conditional. `IfForm` instead
//! overrides [`Callable::call_exprs`] so it receives its arguments UNEVALUATED,
//! evaluates only the test, and then evaluates only the taken branch. This is the
//! SIM-native special-form mechanism -- a callable that drives evaluation of its
//! own argument expressions -- and it needs no kernel change.

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Object, ObjectCompat, RawArgs, Result, Symbol, Value,
};

/// The `if` special form: `(if test then)` or `(if test then else)`.
///
/// Evaluates `test`; if it is truthy, evaluates and returns `then`, otherwise
/// evaluates and returns `else` (or nil when omitted). The untaken branch is
/// never evaluated.
#[derive(Clone, Copy)]
pub struct IfForm;

impl IfForm {
    /// The bare `if` symbol this form registers under.
    pub fn symbol() -> Symbol {
        Symbol::new("if")
    }

    /// Selects the branch expression for a truthy/falsy test, erroring on a bad
    /// arity (`if` takes a test plus one or two branches).
    fn arity_error(count: usize) -> Error {
        Error::Eval(format!(
            "if expects (if test then) or (if test then else), got {count} argument(s)"
        ))
    }
}

impl Object for IfForm {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<special-form if>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IfForm {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for IfForm {
    /// Eager fallback when `if` is applied to already-evaluated values (e.g. via
    /// `apply`): both branches are already values, so just select one. The
    /// lazy-branch semantics live in [`Self::call_exprs`], which the evaluator
    /// uses for a literal `(if ...)` form.
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let values = args.into_vec();
        if values.len() < 2 || values.len() > 3 {
            return Err(Self::arity_error(values.len()));
        }
        if values[0].object().truth(cx)? {
            Ok(values[1].clone())
        } else if let Some(alt) = values.get(2) {
            Ok(alt.clone())
        } else {
            cx.factory().nil()
        }
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let exprs = args.into_exprs();
        if exprs.len() < 2 || exprs.len() > 3 {
            return Err(Self::arity_error(exprs.len()));
        }
        let mut exprs = exprs.into_iter();
        let test = exprs.next().expect("arity checked");
        let then_branch = exprs.next().expect("arity checked");
        let else_branch = exprs.next();

        let taken = if cx.eval_expr(test)?.object().truth(cx)? {
            then_branch
        } else if let Some(else_branch) = else_branch {
            else_branch
        } else {
            return cx.factory().nil();
        };
        cx.eval_expr(taken)
    }
}
