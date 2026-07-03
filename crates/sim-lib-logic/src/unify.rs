//! Standalone unification entry point over the kernel `Expr` graph.
//!
//! [`unify_exprs`] unifies two terms once and reports the result as a kernel
//! `ShapeMatch`, the same accept/reject surface used by the `Shape` protocol;
//! see the [`README`](https://docs.rs/sim-runtime).

use sim_kernel::{Cx, Expr, Result, ShapeMatch};

use crate::{
    env::LogicEnv,
    model::{LogicConfig, OccursCheck},
};

/// Unifies two expressions and reports the result as a kernel `ShapeMatch`.
///
/// On success the returned match is accepting and its captures hold the
/// variable bindings; on a structural mismatch it is a rejecting match. The
/// occurs-check policy comes from `config`.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, Symbol};
/// use sim_lib_logic::{LogicConfig, unify_exprs};
///
/// let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
/// let left = Expr::List(vec![
///     Expr::Symbol(Symbol::new("point")),
///     Expr::Local(Symbol::new("x")),
/// ]);
/// let right = Expr::List(vec![
///     Expr::Symbol(Symbol::new("point")),
///     Expr::Symbol(Symbol::new("origin")),
/// ]);
/// let matched = unify_exprs(&mut cx, &LogicConfig::default(), &left, &right).unwrap();
/// assert!(matched.accepted);
/// ```
pub fn unify_exprs(
    cx: &mut Cx,
    config: &LogicConfig,
    left: &Expr,
    right: &Expr,
) -> Result<ShapeMatch> {
    let mut env = LogicEnv::new();
    let accepted = env.unify(left, right, occurs_check(config))?;
    if accepted {
        env.as_shape_match(cx)
    } else {
        Ok(sim_kernel::ShapeMatch::reject("unification failed"))
    }
}

pub(crate) fn occurs_check(config: &LogicConfig) -> OccursCheck {
    config.occurs_check
}
